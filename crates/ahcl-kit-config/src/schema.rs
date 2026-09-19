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
use crate::{ConfigDocument, ConfigError};
use ahcl_kit_core::{RepoPath, UtcDate};
use std::collections::BTreeSet;
use url::Url;

/// Latest supported configuration schema version.
pub const LATEST_SCHEMA: u32 = 1;

const EVIDENCE_FILE_BYTES: u64 = 2_097_152;
const FILES_PER_PACKAGE: u64 = 64;
const AGGREGATE_EVIDENCE_BYTES: u64 = 536_870_912;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AhclVersion {
    V1_0,
    V1_1,
}

impl AhclVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::V1_0 => "1.0",
            Self::V1_1 => "1.1",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Language {
    Rust,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CargoLockMode {
    Locked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CargoRuleClassification {
    FirstParty,
    ThirdParty,
    Exclude,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CargoRule {
    package: GlobPattern,
    source: Option<GlobPattern>,
    classification: CargoRuleClassification,
}

impl CargoRule {
    pub fn package(&self) -> &str {
        self.package.as_str()
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_ref().map(GlobPattern::as_str)
    }

    pub fn classification(&self) -> CargoRuleClassification {
        self.classification
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
}

impl GlobPattern {
    fn compile(raw: String) -> Result<Self, String> {
        let characters: Vec<_> = raw.chars().collect();
        let mut tokens = Vec::new();
        let mut index = 0;
        while index < characters.len() {
            match characters[index] {
                '*' => {
                    if !matches!(tokens.last(), Some(GlobToken::Star)) {
                        tokens.push(GlobToken::Star);
                    }
                    index += 1;
                }
                '?' => {
                    tokens.push(GlobToken::Any);
                    index += 1;
                }
                '[' => {
                    let Some(close) = characters[index + 1..]
                        .iter()
                        .position(|character| *character == ']')
                    else {
                        return Err("unterminated character class".to_owned());
                    };
                    let close = index + 1 + close;
                    let mut member_index = index + 1;
                    let negative = matches!(characters.get(member_index), Some('!' | '^'));
                    if negative {
                        member_index += 1;
                    }
                    if member_index == close {
                        return Err("empty character class".to_owned());
                    }
                    let mut members = Vec::new();
                    while member_index < close {
                        if member_index + 2 < close && characters[member_index + 1] == '-' {
                            members.push(ClassMember::Range(
                                characters[member_index],
                                characters[member_index + 2],
                            ));
                            member_index += 3;
                        } else {
                            members.push(ClassMember::Single(characters[member_index]));
                            member_index += 1;
                        }
                    }
                    tokens.push(GlobToken::Class { negative, members });
                    index = close + 1;
                }
                character => {
                    tokens.push(GlobToken::Literal(character));
                    index += 1;
                }
            }
        }
        Ok(Self { raw, tokens })
    }

    fn as_str(&self) -> &str {
        &self.raw
    }

    fn matches(&self, value: &str) -> bool {
        let characters: Vec<_> = value.chars().collect();
        let mut previous = vec![false; characters.len() + 1];
        let mut current = vec![false; characters.len() + 1];
        previous[0] = true;

        for token in &self.tokens {
            current.fill(false);
            match token {
                GlobToken::Star => {
                    current[0] = previous[0];
                    for index in 1..=characters.len() {
                        current[index] = previous[index]
                            || (!is_separator(characters[index - 1]) && current[index - 1]);
                    }
                }
                token => {
                    for index in 1..=characters.len() {
                        current[index] =
                            previous[index - 1] && token.matches_character(characters[index - 1]);
                    }
                }
            }
            std::mem::swap(&mut previous, &mut current);
        }
        previous[characters.len()]
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum GlobToken {
    Star,
    Any,
    Class {
        negative: bool,
        members: Vec<ClassMember>,
    },
    Literal(char),
}

impl GlobToken {
    fn matches_character(&self, value: char) -> bool {
        match self {
            Self::Any => !is_separator(value),
            Self::Class { negative, members } => {
                !is_separator(value)
                    && members.iter().any(|member| member.matches(value)) != *negative
            }
            Self::Literal(expected) => value == *expected,
            Self::Star => false,
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
pub struct CargoSettings {
    manifests: Vec<RepoPath>,
    packages: Vec<String>,
    rules: Vec<CargoRule>,
    lock_mode: CargoLockMode,
}

impl CargoSettings {
    pub fn manifests(&self) -> &[RepoPath] {
        &self.manifests
    }

    pub fn packages(&self) -> &[String] {
        &self.packages
    }

    pub fn rules(&self) -> &[CargoRule] {
        &self.rules
    }

    pub fn lock_mode(&self) -> CargoLockMode {
        self.lock_mode
    }

    pub fn classify(&self, package: &str, source: &str) -> CargoRuleClassification {
        self.rules
            .iter()
            .rev()
            .filter(|rule| rule.matches(package, source))
            .map(CargoRule::classification)
            .next()
            .unwrap_or(CargoRuleClassification::ThirdParty)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustSettings {
    cargo: CargoSettings,
}

impl RustSettings {
    pub fn cargo(&self) -> &CargoSettings {
        &self.cargo
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
    special_authorization_channel: String,
}

impl LicenseSettings {
    pub fn version(&self) -> AhclVersion {
        self.version
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectiveConfig {
    schema: u32,
    materials_directory: RepoPath,
    languages: Vec<Language>,
    project: ProjectSettings,
    license: LicenseSettings,
    generation: GenerationSettings,
    rust: RustSettings,
    limits: ConfigLimits,
}

impl EffectiveConfig {
    pub fn resolve(document: &ConfigDocument) -> Result<Self, ConfigError> {
        let ast = document.ast();
        let latest_schema = i64::from(LATEST_SCHEMA);
        let schema = optional_integer(ast, None, "schema")?.unwrap_or(latest_schema);
        if schema != latest_schema {
            return Err(ConfigError::InvalidSchema(format!(
                "unsupported schema version: {schema}"
            )));
        }

        let version = match optional_string(ast, Some("license"), "version")?.as_deref() {
            None | Some("1.1") => AhclVersion::V1_1,
            Some("1.0") => AhclVersion::V1_0,
            Some(value) => {
                return invalid(
                    "license.version",
                    format!("unsupported AHCL version: {value}"),
                );
            }
        };
        let configured_directory = optional_string(ast, None, "materials-directory")?;
        let directory = configured_directory.unwrap_or_else(|| match version {
            AhclVersion::V1_0 => "AHCL".to_owned(),
            AhclVersion::V1_1 => ".ahcl".to_owned(),
        });
        let materials_directory = parse_materials_directory(version, &directory)?;

        let languages = optional_string_list(ast, None, "languages")?
            .unwrap_or_default()
            .into_iter()
            .map(|value| match value.as_str() {
                "rust" => Ok(Language::Rust),
                _ => invalid("languages", format!("unsupported language: {value}")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if languages.iter().collect::<BTreeSet<_>>().len() != languages.len() {
            return invalid("languages", "duplicate language".to_owned());
        }

        let canonical_repository =
            optional_string(ast, Some("project"), "canonical-repository")?.unwrap_or_default();
        if !canonical_repository.is_empty() && !is_absolute_https_url(&canonical_repository) {
            return invalid(
                "project.canonical-repository",
                "must be an absolute HTTPS URL".to_owned(),
            );
        }
        let adoption_date = match optional_string(ast, Some("project"), "adoption-date")? {
            None => None,
            Some(value) if value.is_empty() => None,
            Some(value) => Some(parse_date(&value)?),
        };
        let project = ProjectSettings {
            name: optional_string(ast, Some("project"), "name")?.unwrap_or_default(),
            canonical_repository,
            canonical_branch: optional_string(ast, Some("project"), "canonical-branch")?
                .unwrap_or_else(|| "master".to_owned()),
            right_holders: optional_string_list(ast, Some("project"), "right-holders")?
                .unwrap_or_default(),
            contact: optional_string(ast, Some("project"), "contact")?.unwrap_or_default(),
            adoption_date,
        };
        let license = LicenseSettings {
            version,
            special_authorization_channel: optional_string(
                ast,
                Some("license"),
                "special-authorization-channel",
            )?
            .unwrap_or_default(),
        };
        let generation = GenerationSettings {
            strict_license_files: optional_boolean(
                ast,
                Some("generation"),
                "strict-license-files",
            )?
            .unwrap_or(true),
        };
        let rust = RustSettings {
            cargo: resolve_cargo(ast)?,
        };

        Ok(Self {
            schema: LATEST_SCHEMA,
            materials_directory,
            languages,
            project,
            license,
            generation,
            rust,
            limits: ConfigLimits {
                evidence_file_bytes: EVIDENCE_FILE_BYTES,
                files_per_package: FILES_PER_PACKAGE,
                aggregate_evidence_bytes: AGGREGATE_EVIDENCE_BYTES,
            },
        })
    }

    pub fn schema(&self) -> u32 {
        self.schema
    }

    pub fn materials_directory(&self) -> &RepoPath {
        &self.materials_directory
    }

    pub fn languages(&self) -> &[Language] {
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

    pub fn rust(&self) -> &RustSettings {
        &self.rust
    }

    pub fn limits(&self) -> ConfigLimits {
        self.limits
    }
}

fn resolve_cargo(ast: &DocumentAst) -> Result<CargoSettings, ConfigError> {
    let manifests = optional_string_list(ast, Some("rust.cargo"), "manifests")?
        .unwrap_or_else(|| vec!["Cargo.toml".to_owned()])
        .into_iter()
        .map(|path| parse_repo_path("rust.cargo.manifests", &path))
        .collect::<Result<Vec<_>, _>>()?;
    if manifests.is_empty() {
        return invalid(
            "rust.cargo.manifests",
            "must not be empty when configured".to_owned(),
        );
    }
    let mut portable_manifest_keys = BTreeSet::new();
    for manifest in &manifests {
        if !portable_manifest_keys.insert(manifest.as_str().to_lowercase()) {
            return invalid(
                "rust.cargo.manifests",
                format!("duplicate manifest path: {manifest}"),
            );
        }
    }
    let packages = optional_string_list(ast, Some("rust.cargo"), "packages")?.unwrap_or_default();
    let rules = optional_object_list(ast, Some("rust.cargo"), "rules")?
        .unwrap_or_default()
        .into_iter()
        .map(resolve_rule)
        .collect::<Result<Vec<_>, _>>()?;
    let lock_mode = match optional_string(ast, Some("rust.cargo"), "lock-mode")?.as_deref() {
        None | Some("locked") => CargoLockMode::Locked,
        Some(value) => {
            return invalid(
                "rust.cargo.lock-mode",
                format!("unsupported lock mode: {value}"),
            );
        }
    };
    Ok(CargoSettings {
        manifests,
        packages,
        rules,
        lock_mode,
    })
}

fn resolve_rule(
    fields: std::collections::BTreeMap<String, ScalarValue>,
) -> Result<CargoRule, ConfigError> {
    for key in fields.keys() {
        if !matches!(key.as_str(), "package" | "source" | "classification") {
            return invalid("rust.cargo.rules", format!("unknown rule field: {key}"));
        }
    }
    let package = string_field(&fields, "package")?.ok_or_else(|| ConfigError::InvalidValue {
        path: "rust.cargo.rules.package".to_owned(),
        message: "is required".to_owned(),
    })?;
    let package = GlobPattern::compile(package).map_err(|message| ConfigError::InvalidValue {
        path: "rust.cargo.rules.package".to_owned(),
        message,
    })?;
    let source = string_field(&fields, "source")?
        .map(GlobPattern::compile)
        .transpose()
        .map_err(|message| ConfigError::InvalidValue {
            path: "rust.cargo.rules.source".to_owned(),
            message,
        })?;
    let classification = match string_field(&fields, "classification")?.as_deref() {
        Some("first-party") => CargoRuleClassification::FirstParty,
        Some("third-party") => CargoRuleClassification::ThirdParty,
        Some("exclude") => CargoRuleClassification::Exclude,
        Some(value) => {
            return invalid(
                "rust.cargo.rules.classification",
                format!("unsupported classification: {value}"),
            );
        }
        None => return invalid("rust.cargo.rules.classification", "is required".to_owned()),
    };
    Ok(CargoRule {
        package,
        source,
        classification,
    })
}

fn string_field(
    fields: &std::collections::BTreeMap<String, ScalarValue>,
    key: &str,
) -> Result<Option<String>, ConfigError> {
    match fields.get(key) {
        None => Ok(None),
        Some(ScalarValue::String(value)) => Ok(Some(value.clone())),
        Some(_) => invalid("rust.cargo.rules", format!("{key} must be a string")),
    }
}

fn optional_string(
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

fn optional_integer(
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

fn optional_boolean(
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

fn optional_string_list(
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

fn optional_object_list(
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

fn parse_materials_directory(version: AhclVersion, value: &str) -> Result<RepoPath, ConfigError> {
    let accepted = match version {
        AhclVersion::V1_0 => value == "AHCL",
        AhclVersion::V1_1 => matches!(value, "AHCL" | "licenses/AHCL" | ".AHCL" | ".ahcl"),
    };
    if !accepted {
        return invalid(
            "materials-directory",
            "is not permitted for this AHCL version".to_owned(),
        );
    }
    parse_repo_path("materials-directory", value)
}

fn parse_repo_path(path: &str, value: &str) -> Result<RepoPath, ConfigError> {
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

fn parse_date(value: &str) -> Result<UtcDate, ConfigError> {
    let parts: Vec<_> = value.split('-').collect();
    if parts.len() != 3
        || parts
            .iter()
            .zip([4, 2, 2])
            .any(|(part, length)| part.len() != length)
    {
        return invalid("project.adoption-date", "must use YYYY-MM-DD".to_owned());
    }
    let year = parts[0].parse::<u16>().ok();
    let month = parts[1].parse::<u8>().ok();
    let day = parts[2].parse::<u8>().ok();
    match (year, month, day) {
        (Some(year), Some(month), Some(day)) => {
            UtcDate::new(year, month, day).map_err(|error| ConfigError::InvalidValue {
                path: "project.adoption-date".to_owned(),
                message: error.to_string(),
            })
        }
        _ => invalid("project.adoption-date", "must use YYYY-MM-DD".to_owned()),
    }
}

fn is_absolute_https_url(value: &str) -> bool {
    !value
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
        && Url::parse(value).is_ok_and(|url| url.scheme() == "https" && url.host().is_some())
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
