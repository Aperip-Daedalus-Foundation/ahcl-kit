// crates/ahcl-kit-config/src/schema.rs - Configuration schema validation and resolution.
//
// Copyright (C) 2026 Aperip Daedalus Foundation. All rights reserved.
//
// This file is part of AHCL Kit and is provided under version 1.1 of the
// Aperip Heimdall Commons License (AHCL). The applicable version is also subject
// to the AHCL provisions concerning Continuous AHCL Licensing Segments and
// migration to later official versions.
//
// After having a reasonable opportunity to read AHCL, all applicable Additional
// Restrictions, and all version notices, a person accepts the corresponding terms,
// to the extent permitted by applicable law, by using, copying, modifying, building,
// using this file as a dependency, deploying, distributing, or operating this file
// over a network.
//
// Official AHCL text and public notices:          https://ahcl.aperip.com
// AHCL Materials Directory:                       .ahcl/
// Repository official or recognized AHCL copy:   .ahcl/AHCL-1.1.md
// Project canonical repository:                   https://github.com/Aperip-Daedalus-Foundation/ahcl-kit
// AHCL origin and project notice:                 .ahcl/AHCL-PROJECT-NOTICE.md
// AHCL Version Adoption records:                  .ahcl/AHCL-VERSION-ADOPTION.md
// Complete Corresponding Source and history:      .ahcl/AHCL-SOURCE.md
// Dependencies, Referenced Materials, and licenses:
//                                                    .ahcl/AHCL-DEPENDENCIES.md
//
// SPDX-License-Identifier: LicenseRef-AHCL-1.1

use crate::ast::{DocumentAst, ScalarValue, Value};
use crate::binding::{ConfigValue, LanguageBinding, LanguageContributor, is_config_name};
use crate::{ConfigDocument, ConfigError};
use ahcl_kit_core::{RepoPath, ResolvedPackage, UtcDate};
use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;
use url::Url;

#[path = "schema_glob.rs"]
mod glob;

/// Latest supported configuration schema version.
pub const LATEST_SCHEMA: u32 = 1;

const EVIDENCE_FILE_BYTES: u64 = 2_097_152;
const FILES_PER_PACKAGE: u64 = 64;
const AGGREGATE_EVIDENCE_BYTES: u64 = 536_870_912;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AhclVersion {
    V1_0,
    V1_1,
    V1_2,
}

impl AhclVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V1_0 => "1.0",
            Self::V1_1 => "1.1",
            Self::V1_2 => "1.2",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackageRuleClassification {
    FirstParty,
    ThirdParty,
    Exclude,
}

impl PackageRuleClassification {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstParty => "first-party",
            Self::ThirdParty => "third-party",
            Self::Exclude => "exclude",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageRule {
    package: GlobPattern,
    source: Option<GlobPattern>,
    classification: PackageRuleClassification,
}

impl PackageRule {
    pub fn package(&self) -> &str {
        self.package.as_str()
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_ref().map(GlobPattern::as_str)
    }

    pub fn classification(&self) -> PackageRuleClassification {
        self.classification
    }

    pub fn applies(&self, package: &str, source: &str) -> bool {
        self.matches(package, source)
    }

    pub fn parse(
        path: &str,
        package_name: bool,
        fields: std::collections::BTreeMap<String, ScalarValue>,
    ) -> Result<Self, ConfigError> {
        let mode = if package_name {
            PackagePatternMode::PackageName
        } else {
            PackagePatternMode::Path
        };
        resolve_package_rule(path, mode, fields)
    }

    pub fn classify_list(rules: &[Self], package: &str, source: &str) -> PackageRuleClassification {
        rules
            .iter()
            .rev()
            .find(|rule| rule.applies(package, source))
            .map(Self::classification)
            .unwrap_or(PackageRuleClassification::ThirdParty)
    }

    pub fn resolved_value(&self) -> ConfigValue {
        ConfigValue::Object(vec![
            (
                "package".to_owned(),
                ConfigValue::String(self.package().to_owned()),
            ),
            (
                "source".to_owned(),
                self.source()
                    .map(|value| ConfigValue::String(value.to_owned()))
                    .unwrap_or(ConfigValue::Null),
            ),
            (
                "classification".to_owned(),
                ConfigValue::String(self.classification().as_str().to_owned()),
            ),
        ])
    }

    fn matches(&self, package: &str, source: &str) -> bool {
        self.package.matches(package)
            && self
                .source
                .as_ref()
                .is_none_or(|pattern| pattern.matches(source))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GlobPattern {
    raw: String,
    tokens: Vec<GlobToken>,
    /// Package names are matched as a single string, so `/` is not a separator.
    package_name: bool,
}

impl GlobPattern {
    fn compile(raw: String) -> Result<Self, String> {
        Self::compile_with(raw, false)
    }

    fn compile_package_name(raw: String) -> Result<Self, String> {
        Self::compile_with(raw, true)
    }

    fn compile_with(raw: String, package_name: bool) -> Result<Self, String> {
        let characters: Vec<_> = raw.chars().collect();
        let mut tokens = Vec::new();
        let mut index = 0;
        while index < characters.len() {
            index = glob::push_glob_token(&characters, &mut tokens, index, package_name)?;
        }
        Ok(Self {
            raw,
            tokens,
            package_name,
        })
    }

    fn as_str(&self) -> &str {
        &self.raw
    }

    fn matches(&self, value: &str) -> bool {
        let characters: Vec<_> = value.chars().collect();
        let mut active = vec![false; characters.len() + 1];
        active[0] = true;
        for token in &self.tokens {
            active = glob::advance_glob(self, token, &characters, &active);
        }
        active[characters.len()]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum GlobToken {
    Star,
    Globstar,
    Any,
    Class {
        negative: bool,
        members: Vec<ClassMember>,
    },
    Literal(char),
}

impl GlobToken {
    fn matches_character(&self, value: char, separators: bool) -> bool {
        let separator = separators && is_separator(value);
        match self {
            Self::Any => !separator,
            Self::Class { negative, members } => {
                !separator && members.iter().any(|member| member.matches(value)) != *negative
            }
            Self::Literal(expected) => value == *expected,
            Self::Star | Self::Globstar => false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ClassMember {
    Single(char),
    Range(char, char),
}

impl ClassMember {
    fn matches(&self, value: char) -> bool {
        match self {
            Self::Single(expected) => value == *expected,
            Self::Range(start, end) => *start <= value && value <= *end,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectSettings {
    name: String,
    canonical_repository: String,
    canonical_branch: String,
    right_holders: Vec<String>,
    contact: String,
    adoption_date: Option<UtcDate>,
}

impl ProjectSettings {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn canonical_repository(&self) -> &str {
        &self.canonical_repository
    }

    pub fn canonical_branch(&self) -> &str {
        &self.canonical_branch
    }

    pub fn right_holders(&self) -> &[String] {
        &self.right_holders
    }

    pub fn contact(&self) -> &str {
        &self.contact
    }

    pub fn adoption_date(&self) -> Option<UtcDate> {
        self.adoption_date
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LicenseSettings {
    version: AhclVersion,
    enabled: bool,
    covered_scope: String,
    special_authorization_channel: String,
}

impl LicenseSettings {
    pub fn version(&self) -> AhclVersion {
        self.version
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn covered_scope(&self) -> &str {
        &self.covered_scope
    }

    pub fn special_authorization_channel(&self) -> &str {
        &self.special_authorization_channel
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GenerationSettings {
    strict_license_files: bool,
}

impl GenerationSettings {
    pub fn strict_license_files(&self) -> bool {
        self.strict_license_files
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigLimits {
    evidence_file_bytes: u64,
    files_per_package: u64,
    aggregate_evidence_bytes: u64,
}

impl ConfigLimits {
    pub fn evidence_file_bytes(&self) -> u64 {
        self.evidence_file_bytes
    }

    pub fn files_per_package(&self) -> u64 {
        self.files_per_package
    }

    pub fn aggregate_evidence_bytes(&self) -> u64 {
        self.aggregate_evidence_bytes
    }
}

/// Identity overrides contributed by one ecosystem component.
pub struct ComponentIdentity {
    pub path: String,
    pub package: String,
    pub enabled: bool,
    pub centralized: bool,
    pub materials_directory: Option<RepoPath>,
    pub license_version: Option<AhclVersion>,
    pub covered_scope: Option<String>,
    pub right_holders: Option<Vec<String>>,
    pub canonical_repository: Option<String>,
    pub canonical_branch: Option<String>,
    pub contact: Option<String>,
    pub adoption_date: Option<UtcDate>,
    pub special_authorization_channel: Option<String>,
}

#[derive(Clone)]
pub struct EffectiveConfig {
    schema: u32,
    materials_directory: RepoPath,
    languages: Vec<String>,
    project: ProjectSettings,
    license: LicenseSettings,
    generation: GenerationSettings,
    bindings: Vec<Arc<dyn LanguageBinding>>,
    limits: ConfigLimits,
}

impl EffectiveConfig {
    pub fn resolve(document: &ConfigDocument) -> Result<Self, ConfigError> {
        let ast = document.ast();
        let version = resolve_version(ast)?;
        let materials_directory = resolve_directory(ast, version)?;
        let requested = requested_languages(ast)?;
        let (languages, bindings) = load_bindings(document, &requested)?;
        let project = resolve_project(ast)?;
        let license = resolve_license(ast, version)?;
        let generation = resolve_generation(ast)?;
        Ok(Self {
            schema: LATEST_SCHEMA,
            materials_directory,
            languages,
            project,
            license,
            generation,
            bindings,
            limits: default_limits(),
        })
    }

    pub fn schema(&self) -> u32 {
        self.schema
    }

    pub fn materials_directory(&self) -> &RepoPath {
        &self.materials_directory
    }

    pub fn languages(&self) -> &[String] {
        &self.languages
    }

    pub fn project(&self) -> &ProjectSettings {
        &self.project
    }

    pub fn license(&self) -> &LicenseSettings {
        &self.license
    }

    pub fn generation(&self) -> GenerationSettings {
        self.generation
    }

    pub fn limits(&self) -> ConfigLimits {
        self.limits
    }

    pub fn bindings(&self) -> &[Arc<dyn LanguageBinding>] {
        &self.bindings
    }

    pub fn adapter_names(&self) -> Vec<&'static str> {
        self.languages
            .iter()
            .filter_map(|language| {
                self.bindings
                    .iter()
                    .find(|binding| binding.language_id() == language)
                    .map(|binding| binding.display_name())
            })
            .collect()
    }

    pub fn adapter_summary_lines(&self) -> Vec<String> {
        self.bindings
            .iter()
            .flat_map(|binding| binding.summary_lines())
            .collect()
    }

    pub fn package_field_labels(&self, package: &ResolvedPackage) -> (&'static str, &'static str) {
        if let Some(binding) = self
            .bindings
            .iter()
            .find(|binding| binding.owns_package(package))
        {
            return (binding.package_id_label(), binding.package_source_label());
        }
        if let Some(binding) = self
            .bindings
            .iter()
            .find(|binding| binding.is_package_fallback())
        {
            return (binding.package_id_label(), binding.package_source_label());
        }
        ("Package ID", "Source")
    }

    pub fn resolved_sections(&self) -> Vec<(String, crate::ConfigValue)> {
        self.bindings
            .iter()
            .map(|binding| binding.resolved_entry())
            .collect()
    }

    pub fn replace_binding(&mut self, binding: Box<dyn LanguageBinding>) {
        let language_id = binding.language_id();
        let replacement = Arc::<dyn LanguageBinding>::from(binding);
        if let Some(slot) = self
            .bindings
            .iter_mut()
            .find(|item| item.language_id() == language_id)
        {
            *slot = replacement;
            return;
        }
        self.bindings.push(replacement);
    }

    pub fn for_component_identity(&self, identity: ComponentIdentity) -> Result<Self, ConfigError> {
        let version = identity.license_version.unwrap_or(self.license.version);
        let materials_directory = identity
            .materials_directory
            .clone()
            .unwrap_or_else(|| self.materials_directory.clone());
        if identity.centralized
            && (self.license.version != AhclVersion::V1_2
                || version != AhclVersion::V1_2
                || materials_directory != self.materials_directory)
        {
            return invalid(
                identity.path,
                "centralized components require AHCL 1.2 and the parent materials directory"
                    .to_owned(),
            );
        }
        let right_holders = component_right_holders(&identity, &self.project.right_holders)?;
        let project = narrowed_project(&self.project, &identity, right_holders);
        let license = LicenseSettings {
            version,
            enabled: identity.enabled,
            covered_scope: identity
                .covered_scope
                .clone()
                .unwrap_or_else(|| identity.package.clone()),
            special_authorization_channel: identity
                .special_authorization_channel
                .clone()
                .unwrap_or_else(|| self.license.special_authorization_channel.clone()),
        };
        Ok(Self {
            schema: self.schema,
            materials_directory,
            languages: self.languages.clone(),
            project,
            license,
            generation: self.generation,
            bindings: self.bindings.clone(),
            limits: self.limits,
        })
    }

    pub fn with_covered_scope(&self, covered_scope: &str) -> Self {
        let mut cloned = self.clone();
        cloned.license.covered_scope = covered_scope.to_owned();
        cloned
    }

    pub fn with_license_enabled(&self, enabled: bool) -> Self {
        let mut cloned = self.clone();
        cloned.license.enabled = enabled;
        cloned
    }
}

impl fmt::Debug for EffectiveConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EffectiveConfig")
            .field("schema", &self.schema)
            .field("languages", &self.languages)
            .field(
                "bindings",
                &self
                    .bindings
                    .iter()
                    .map(|binding| binding.language_id())
                    .collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

type LoadedLanguages = (Vec<String>, Vec<Arc<dyn LanguageBinding>>);

fn resolve_version(ast: &DocumentAst) -> Result<AhclVersion, ConfigError> {
    require_latest_schema(ast)?;
    match optional_string(ast, Some("license"), "version")?.as_deref() {
        None | Some("1.1") => Ok(AhclVersion::V1_1),
        Some("1.0") => Ok(AhclVersion::V1_0),
        Some("1.2") => Ok(AhclVersion::V1_2),
        Some(value) => invalid(
            "license.version",
            format!("unsupported AHCL version: {value}"),
        ),
    }
}

fn require_latest_schema(ast: &DocumentAst) -> Result<(), ConfigError> {
    let latest_schema = i64::from(LATEST_SCHEMA);
    let schema = optional_integer(ast, None, "schema")?.unwrap_or(latest_schema);
    if schema != latest_schema {
        return Err(ConfigError::InvalidSchema(format!(
            "unsupported schema version: {schema}"
        )));
    }
    Ok(())
}

fn resolve_directory(ast: &DocumentAst, version: AhclVersion) -> Result<RepoPath, ConfigError> {
    let configured = optional_string(ast, None, "materials-directory")?;
    let directory = configured.unwrap_or_else(|| default_directory(version));
    parse_materials_directory(version, &directory)
}

fn default_directory(version: AhclVersion) -> String {
    match version {
        AhclVersion::V1_0 => "AHCL".to_owned(),
        AhclVersion::V1_1 | AhclVersion::V1_2 => ".ahcl".to_owned(),
    }
}

fn requested_languages(ast: &DocumentAst) -> Result<Vec<String>, ConfigError> {
    Ok(optional_string_list(ast, None, "languages")?.unwrap_or_default())
}

fn resolve_project(ast: &DocumentAst) -> Result<ProjectSettings, ConfigError> {
    let identity = project_identity(ast)?;
    Ok(ProjectSettings {
        name: identity.name,
        canonical_repository: identity.repository,
        canonical_branch: identity.branch,
        right_holders: identity.holders,
        contact: identity.contact,
        adoption_date: resolve_adoption_date(ast)?,
    })
}

struct ProjectIdentityFields {
    name: String,
    repository: String,
    branch: String,
    holders: Vec<String>,
    contact: String,
}

fn project_identity(ast: &DocumentAst) -> Result<ProjectIdentityFields, ConfigError> {
    Ok(ProjectIdentityFields {
        name: project_string(ast, "name")?,
        repository: canonical_repository(ast)?,
        branch: optional_string(ast, Some("project"), "canonical-branch")?
            .unwrap_or_else(|| "master".to_owned()),
        holders: optional_string_list(ast, Some("project"), "right-holders")?.unwrap_or_default(),
        contact: project_string(ast, "contact")?,
    })
}

fn project_string(ast: &DocumentAst, key: &str) -> Result<String, ConfigError> {
    Ok(optional_string(ast, Some("project"), key)?.unwrap_or_default())
}

fn canonical_repository(ast: &DocumentAst) -> Result<String, ConfigError> {
    let value = project_string(ast, "canonical-repository")?;
    if !value.is_empty() && !is_absolute_https_url(&value) {
        return invalid(
            "project.canonical-repository",
            "must be an absolute HTTPS URL".to_owned(),
        );
    }
    Ok(value)
}

fn resolve_adoption_date(ast: &DocumentAst) -> Result<Option<UtcDate>, ConfigError> {
    match optional_string(ast, Some("project"), "adoption-date")? {
        None => Ok(None),
        Some(value) if value.is_empty() => Ok(None),
        Some(value) => Ok(Some(parse_date_at(&value, "project.adoption-date")?)),
    }
}

fn resolve_license(
    ast: &DocumentAst,
    version: AhclVersion,
) -> Result<LicenseSettings, ConfigError> {
    Ok(LicenseSettings {
        version,
        enabled: optional_boolean(ast, Some("license"), "enabled")?.unwrap_or(true),
        covered_scope: covered_scope_setting(ast)?,
        special_authorization_channel: optional_string(
            ast,
            Some("license"),
            "special-authorization-channel",
        )?
        .unwrap_or_default(),
    })
}

fn covered_scope_setting(ast: &DocumentAst) -> Result<String, ConfigError> {
    Ok(optional_string(ast, Some("license"), "covered-scope")?
        .map(|value| validate_scope("license.covered-scope", value))
        .transpose()?
        .unwrap_or_default())
}

fn resolve_generation(ast: &DocumentAst) -> Result<GenerationSettings, ConfigError> {
    Ok(GenerationSettings {
        strict_license_files: optional_boolean(ast, Some("generation"), "strict-license-files")?
            .unwrap_or(true),
    })
}

fn default_limits() -> ConfigLimits {
    ConfigLimits {
        evidence_file_bytes: EVIDENCE_FILE_BYTES,
        files_per_package: FILES_PER_PACKAGE,
        aggregate_evidence_bytes: AGGREGATE_EVIDENCE_BYTES,
    }
}

fn load_bindings(
    document: &ConfigDocument,
    requested: &[String],
) -> Result<LoadedLanguages, ConfigError> {
    let known = known_languages(document)?;
    let languages = selected_languages(requested, &known)?;
    let bindings = language_bindings(document, &languages)?;
    Ok((languages, bindings))
}

fn known_languages(document: &ConfigDocument) -> Result<BTreeSet<&'static str>, ConfigError> {
    let mut known = BTreeSet::new();
    for contributor in document.contributors() {
        record_language(&mut known, contributor.language_id())?;
    }
    Ok(known)
}

fn record_language(
    known: &mut BTreeSet<&'static str>,
    language_id: &'static str,
) -> Result<(), ConfigError> {
    if !is_config_name(language_id) || !known.insert(language_id) {
        return invalid(
            "languages",
            format!("duplicate language contributor: {language_id}"),
        );
    }
    Ok(())
}

fn selected_languages(
    requested: &[String],
    known: &BTreeSet<&'static str>,
) -> Result<Vec<String>, ConfigError> {
    let mut languages = Vec::new();
    let mut selected: BTreeSet<&str> = BTreeSet::new();
    for language in requested {
        push_selected_language(&mut languages, &mut selected, known, language)?;
    }
    Ok(languages)
}

fn push_selected_language<'a>(
    languages: &mut Vec<String>,
    selected: &mut BTreeSet<&'a str>,
    known: &BTreeSet<&'static str>,
    language: &'a String,
) -> Result<(), ConfigError> {
    if !selected.insert(language.as_str()) {
        return invalid("languages", "duplicate language".to_owned());
    }
    if !known.contains(language.as_str()) {
        return invalid("languages", format!("unsupported language: {language}"));
    }
    languages.push(language.clone());
    Ok(())
}

fn language_bindings(
    document: &ConfigDocument,
    languages: &[String],
) -> Result<Vec<Arc<dyn LanguageBinding>>, ConfigError> {
    let selected = languages
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut bindings = Vec::new();
    let mut resolved_keys = BTreeSet::new();
    let mut fallbacks = 0_usize;
    for contributor in document.contributors() {
        fallbacks += push_language_binding(
            document,
            *contributor,
            &selected,
            &mut bindings,
            &mut resolved_keys,
        )?;
    }
    if fallbacks > 1 {
        return invalid(
            "languages",
            "multiple package identity fallbacks".to_owned(),
        );
    }
    Ok(bindings)
}

fn push_language_binding(
    document: &ConfigDocument,
    contributor: &'static dyn LanguageContributor,
    selected: &BTreeSet<&str>,
    bindings: &mut Vec<Arc<dyn LanguageBinding>>,
    resolved_keys: &mut BTreeSet<String>,
) -> Result<usize, ConfigError> {
    let enabled = selected.contains(contributor.language_id());
    let binding = contributor.load(document, enabled)?;
    let (key, _) = binding.resolved_entry();
    if !resolved_keys.insert(key) {
        return invalid(
            "languages",
            format!(
                "duplicate resolved configuration from {}",
                contributor.language_id()
            ),
        );
    }
    let fallback = usize::from(binding.is_package_fallback());
    bindings.push(Arc::from(binding));
    Ok(fallback)
}

fn component_right_holders(
    identity: &ComponentIdentity,
    parent: &[String],
) -> Result<Vec<String>, ConfigError> {
    match &identity.right_holders {
        Some(values) if values.is_empty() => invalid(
            format!("{}.right-holders", identity.path),
            "must not be explicitly empty".to_owned(),
        ),
        Some(values) => Ok(values.clone()),
        None => Ok(parent.to_vec()),
    }
}

fn narrowed_project(
    parent: &ProjectSettings,
    identity: &ComponentIdentity,
    right_holders: Vec<String>,
) -> ProjectSettings {
    ProjectSettings {
        name: identity.package.clone(),
        canonical_repository: identity
            .canonical_repository
            .clone()
            .unwrap_or_else(|| parent.canonical_repository.clone()),
        canonical_branch: identity
            .canonical_branch
            .clone()
            .unwrap_or_else(|| parent.canonical_branch.clone()),
        right_holders,
        contact: identity
            .contact
            .clone()
            .unwrap_or_else(|| parent.contact.clone()),
        adoption_date: identity.adoption_date.or(parent.adoption_date),
    }
}

pub(crate) fn fields_for_section(
    ast: &DocumentAst,
    section: &str,
) -> std::collections::BTreeMap<String, ScalarValue> {
    ast.assignments
        .iter()
        .filter(|assignment| assignment.section.as_deref() == Some(section))
        .filter_map(|assignment| match &assignment.value {
            Value::Scalar(value) => Some((assignment.key.clone(), value.clone())),
            _ => None,
        })
        .collect()
}

pub fn parse_version(value: &str, path: &str) -> Result<AhclVersion, ConfigError> {
    match value {
        "1.0" => Ok(AhclVersion::V1_0),
        "1.1" => Ok(AhclVersion::V1_1),
        "1.2" => Ok(AhclVersion::V1_2),
        _ => invalid(
            path.to_owned(),
            format!("unsupported AHCL version: {value}"),
        ),
    }
}

pub fn validate_scope(path: &str, value: String) -> Result<String, ConfigError> {
    if value.is_empty() || value.contains(['\n', '\r']) {
        return invalid(
            path.to_owned(),
            "must be a non-empty single-line string when configured".to_owned(),
        );
    }
    Ok(value)
}

pub fn parse_date_at(value: &str, path: &str) -> Result<UtcDate, ConfigError> {
    let parts: Vec<_> = value.split('-').collect();
    if parts.len() != 3
        || parts
            .iter()
            .zip([4, 2, 2])
            .any(|(part, length)| part.len() != length)
    {
        return invalid(path.to_owned(), "must use YYYY-MM-DD".to_owned());
    }
    match (
        parts[0].parse::<u16>().ok(),
        parts[1].parse::<u8>().ok(),
        parts[2].parse::<u8>().ok(),
    ) {
        (Some(year), Some(month), Some(day)) => {
            UtcDate::new(year, month, day).map_err(|error| ConfigError::InvalidValue {
                path: path.to_owned(),
                message: error.to_string(),
            })
        }
        _ => invalid(path.to_owned(), "must use YYYY-MM-DD".to_owned()),
    }
}

pub fn is_commit_revision(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PackagePatternMode {
    Path,
    PackageName,
}

fn resolve_package_rule(
    prefix: &str,
    package_mode: PackagePatternMode,
    fields: std::collections::BTreeMap<String, ScalarValue>,
) -> Result<PackageRule, ConfigError> {
    reject_unknown_rule_fields(prefix, &fields)?;
    let package = required_rule_string(prefix, &fields, "package")?;
    let package = compile_package_pattern(prefix, package_mode, package)?;
    let source = compile_optional_source(prefix, &fields)?;
    let classification = rule_classification(prefix, &fields)?;
    Ok(PackageRule {
        package,
        source,
        classification,
    })
}

fn reject_unknown_rule_fields(
    prefix: &str,
    fields: &std::collections::BTreeMap<String, ScalarValue>,
) -> Result<(), ConfigError> {
    for key in fields.keys() {
        if !known_rule_field(key) {
            return invalid(prefix, format!("unknown rule field: {key}"));
        }
    }
    Ok(())
}

fn known_rule_field(key: &str) -> bool {
    matches!(key, "package" | "source" | "classification")
}

fn required_rule_string(
    prefix: &str,
    fields: &std::collections::BTreeMap<String, ScalarValue>,
    key: &str,
) -> Result<String, ConfigError> {
    rule_string(prefix, fields, key)?.ok_or_else(|| ConfigError::InvalidValue {
        path: format!("{prefix}.{key}"),
        message: "is required".to_owned(),
    })
}

fn compile_package_pattern(
    prefix: &str,
    package_mode: PackagePatternMode,
    package: String,
) -> Result<GlobPattern, ConfigError> {
    let compiled = match package_mode {
        PackagePatternMode::Path => GlobPattern::compile(package),
        PackagePatternMode::PackageName => GlobPattern::compile_package_name(package),
    };
    compiled.map_err(|message| ConfigError::InvalidValue {
        path: format!("{prefix}.package"),
        message,
    })
}

fn compile_optional_source(
    prefix: &str,
    fields: &std::collections::BTreeMap<String, ScalarValue>,
) -> Result<Option<GlobPattern>, ConfigError> {
    rule_string(prefix, fields, "source")?
        .map(GlobPattern::compile)
        .transpose()
        .map_err(|message| ConfigError::InvalidValue {
            path: format!("{prefix}.source"),
            message,
        })
}

fn rule_classification(
    prefix: &str,
    fields: &std::collections::BTreeMap<String, ScalarValue>,
) -> Result<PackageRuleClassification, ConfigError> {
    match rule_string(prefix, fields, "classification")?.as_deref() {
        Some("first-party") => Ok(PackageRuleClassification::FirstParty),
        Some("third-party") => Ok(PackageRuleClassification::ThirdParty),
        Some("exclude") => Ok(PackageRuleClassification::Exclude),
        Some(value) => invalid(
            format!("{prefix}.classification"),
            format!("unsupported classification: {value}"),
        ),
        None => invalid(format!("{prefix}.classification"), "is required".to_owned()),
    }
}

fn rule_string(
    prefix: &str,
    fields: &std::collections::BTreeMap<String, ScalarValue>,
    key: &str,
) -> Result<Option<String>, ConfigError> {
    match fields.get(key) {
        None => Ok(None),
        Some(ScalarValue::String(value)) => Ok(Some(value.clone())),
        Some(_) => invalid(prefix, format!("{key} must be a string")),
    }
}

pub(crate) fn optional_string(
    ast: &DocumentAst,
    section: Option<&str>,
    key: &str,
) -> Result<Option<String>, ConfigError> {
    match ast
        .assignment(section, key)
        .map(|assignment| &assignment.value)
    {
        None => Ok(None),
        Some(Value::Scalar(ScalarValue::String(value))) => Ok(Some(value.clone())),
        Some(_) => invalid(path(section, key), "must be a string".to_owned()),
    }
}

pub(crate) fn optional_integer(
    ast: &DocumentAst,
    section: Option<&str>,
    key: &str,
) -> Result<Option<i64>, ConfigError> {
    match ast
        .assignment(section, key)
        .map(|assignment| &assignment.value)
    {
        None => Ok(None),
        Some(Value::Scalar(ScalarValue::Integer(value))) => Ok(Some(*value)),
        Some(_) => invalid(path(section, key), "must be an integer".to_owned()),
    }
}

pub(crate) fn optional_boolean(
    ast: &DocumentAst,
    section: Option<&str>,
    key: &str,
) -> Result<Option<bool>, ConfigError> {
    match ast
        .assignment(section, key)
        .map(|assignment| &assignment.value)
    {
        None => Ok(None),
        Some(Value::Scalar(ScalarValue::Boolean(value))) => Ok(Some(*value)),
        Some(_) => invalid(path(section, key), "must be a boolean".to_owned()),
    }
}

pub(crate) fn optional_string_list(
    ast: &DocumentAst,
    section: Option<&str>,
    key: &str,
) -> Result<Option<Vec<String>>, ConfigError> {
    match ast
        .assignment(section, key)
        .map(|assignment| &assignment.value)
    {
        None => Ok(None),
        Some(Value::EmptyList) => Ok(Some(Vec::new())),
        Some(Value::ScalarList(values)) => values
            .iter()
            .map(|value| match value {
                ScalarValue::String(value) => Ok(value.clone()),
                _ => invalid(path(section, key), "items must be strings".to_owned()),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => invalid(path(section, key), "must be a list".to_owned()),
    }
}

pub(crate) fn optional_object_list(
    ast: &DocumentAst,
    section: Option<&str>,
    key: &str,
) -> Result<Option<Vec<std::collections::BTreeMap<String, ScalarValue>>>, ConfigError> {
    match ast
        .assignment(section, key)
        .map(|assignment| &assignment.value)
    {
        None | Some(Value::EmptyList) => Ok(None),
        Some(Value::ObjectList(values)) => Ok(Some(values.clone())),
        Some(_) => invalid(path(section, key), "must be an object list".to_owned()),
    }
}

pub fn parse_materials_directory(
    version: AhclVersion,
    value: &str,
) -> Result<RepoPath, ConfigError> {
    let accepted = match version {
        AhclVersion::V1_0 => value == "AHCL",
        AhclVersion::V1_1 | AhclVersion::V1_2 => {
            matches!(value, "AHCL" | "licenses/AHCL" | ".AHCL" | ".ahcl")
        }
    };
    if !accepted {
        return invalid(
            "materials-directory",
            "is not permitted for this AHCL version".to_owned(),
        );
    }
    parse_repo_path("materials-directory", value)
}

pub fn parse_repo_path(path: &str, value: &str) -> Result<RepoPath, ConfigError> {
    if value.contains('\\') {
        return invalid(
            path,
            "must use '/' as its repository-relative separator".to_owned(),
        );
    }
    RepoPath::parse(value).map_err(|error| ConfigError::InvalidValue {
        path: path.to_owned(),
        message: error.to_string(),
    })
}

pub fn is_absolute_https_url(value: &str) -> bool {
    !value
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
        && Url::parse(value).is_ok_and(|url| url.scheme() == "https" && url.host().is_some())
}

pub fn is_secure_https_url(value: &str) -> bool {
    !value
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
        && Url::parse(value).is_ok_and(|url| {
            url.scheme() == "https"
                && url.host().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.port_or_known_default() == Some(443)
        })
}

fn path(section: Option<&str>, key: &str) -> String {
    section.map_or_else(|| key.to_owned(), |section| format!("{section}.{key}"))
}

fn invalid<T>(path: impl Into<String>, message: String) -> Result<T, ConfigError> {
    Err(ConfigError::InvalidValue {
        path: path.into(),
        message,
    })
}

fn is_separator(character: char) -> bool {
    matches!(character, '/' | '\\')
}
