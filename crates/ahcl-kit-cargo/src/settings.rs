// crates/ahcl-kit-cargo/src/settings.rs - Cargo configuration owned by the Cargo adapter.
//
// Copyright (C) 2026 Aperip Daedalus Foundation. All rights reserved.
//
// The AHCL-covered material identified below forms part of
// AHCL Kit and is provided under version 1.2 of the
// Aperip Heimdall Commons License (AHCL). The applicable version is also subject
// to the AHCL provisions concerning Continuous AHCL Licensing Segments and
// migration to later official versions.
//
// AHCL-covered portions: the whole file
//
// Subject to Section 3.1 of AHCL, after having a reasonable opportunity to
// read AHCL, all applicable Additional Restrictions, and all version notices,
// a person accepts the corresponding terms by engaging in Use of the
// AHCL-covered material identified above. Any separate or affirmative assent
// required by applicable mandatory law must still be obtained.
//
// Official AHCL text and public notices:          https://ahcl.aperip.com
// Applicable LICENSE:                             LICENSE
// Paths below are relative to the directory containing that LICENSE.
// AHCL Materials Directory:                       .ahcl/
// Repository official or recognized AHCL copy:   .ahcl/AHCL-1.2.md
// Project canonical repository:                   https://github.com/Aperip-Daedalus-Foundation/ahcl-kit
// AHCL origin and project notice:                 .ahcl/AHCL-PROJECT-NOTICE.md
// AHCL Version Adoption records:                  .ahcl/AHCL-VERSION-ADOPTION.md
// Complete Corresponding Source and history:      .ahcl/AHCL-SOURCE.md
// Dependencies, Referenced Materials, and licenses:
//                                                    .ahcl/AHCL-DEPENDENCIES.md
//
// SPDX-License-Identifier: LicenseRef-AHCL-1.2

use ahcl_kit_config::{
    AhclVersion, ComponentIdentity, ConfigDocument, ConfigError, ConfigValue, EffectiveConfig,
    LanguageBinding, LanguageContributor, PackageRule, PackageRuleClassification, ScalarValue,
    inline_text, is_commit_revision, is_config_name, is_secure_https_url, parse_date_at,
    parse_materials_directory, parse_repo_path, parse_version, validate_scope,
};
use ahcl_kit_core::RepoPath;
use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};

const EVIDENCE_PREFIX: &str = "rust.cargo.evidence.";
const COMPONENT_PREFIX: &str = "rust.cargo.component.";

pub static CONTRIBUTOR: CargoContributor = CargoContributor;

#[derive(Clone, Copy, Debug)]
pub struct CargoContributor;

impl LanguageContributor for CargoContributor {
    fn language_id(&self) -> &'static str {
        "rust"
    }

    fn display_name(&self) -> &'static str {
        "Rust"
    }

    fn skeleton_hint(&self) -> &'static str {
        "\n# To enable the Rust adapter, replace `languages = []` above with:\n# languages:\n#   - \"rust\"\n#\n# Then add:\n# [rust.cargo]\n# manifests:\n#   - \"Cargo.toml\"\n# packages = []\n# rules = []\n#\n"
    }

    fn owns_section(&self, section: &str) -> bool {
        section == "rust.cargo"
            || named_suffix(section, EVIDENCE_PREFIX)
            || named_suffix(section, COMPONENT_PREFIX)
    }

    fn known_field(&self, section: &str, key: &str) -> bool {
        if section == "rust.cargo" {
            return cargo_root_field(key);
        }
        if named_suffix(section, EVIDENCE_PREFIX) {
            return evidence_field_name(key);
        }
        named_suffix(section, COMPONENT_PREFIX) && component_field_name(key)
    }

    fn load(
        &self,
        document: &ConfigDocument,
        enabled: bool,
    ) -> Result<Box<dyn LanguageBinding>, ConfigError> {
        Ok(Box::new(CargoBinding {
            settings: resolve_cargo(document)?,
            enabled,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CargoBinding {
    settings: CargoSettings,
    enabled: bool,
}

impl CargoBinding {
    pub fn settings(&self) -> &CargoSettings {
        &self.settings
    }

    pub fn from_config(config: &EffectiveConfig) -> Option<&Self> {
        config
            .bindings()
            .iter()
            .find_map(|binding| binding.as_any().downcast_ref::<Self>())
    }

    pub fn with_selected_package(&self, package: &str) -> Self {
        let mut settings = self.settings.clone();
        settings.components.clear();
        settings.packages = vec![package.to_owned()];
        Self {
            settings,
            enabled: self.enabled,
        }
    }

    pub fn component_config(
        &self,
        config: &EffectiveConfig,
        component: &CargoComponent,
    ) -> Result<EffectiveConfig, ConfigError> {
        let path = format!("rust.cargo.component.{}", component.id());
        let mut narrowed = config.for_component_identity(ComponentIdentity {
            path,
            package: component.package().to_owned(),
            enabled: component.enabled(),
            centralized: component.layout() == ComponentLayout::Centralized,
            materials_directory: component.materials_directory().cloned(),
            license_version: component.license_version(),
            covered_scope: component.covered_scope().map(str::to_owned),
            right_holders: component.right_holders().map(|values| values.to_vec()),
            canonical_repository: component.canonical_repository().map(str::to_owned),
            canonical_branch: component.canonical_branch().map(str::to_owned),
            contact: component.contact().map(str::to_owned),
            adoption_date: component.adoption_date(),
            special_authorization_channel: component
                .special_authorization_channel()
                .map(str::to_owned),
        })?;
        narrowed.replace_binding(Box::new(self.with_selected_package(component.package())));
        Ok(narrowed)
    }
}

impl LanguageBinding for CargoBinding {
    fn language_id(&self) -> &'static str {
        "rust"
    }

    fn display_name(&self) -> &'static str {
        "Rust"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn clone_box(&self) -> Box<dyn LanguageBinding> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let manifests = self
            .settings
            .manifests
            .iter()
            .map(|path| format!("`{}`", path.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("- Cargo manifests: {manifests}"));
        if self.settings.packages.is_empty() {
            lines.push("- Cargo package selection: All configured workspace roots.".to_owned());
        } else {
            let packages = self
                .settings
                .packages
                .iter()
                .map(|package| format!("`{}`", inline_text(package)))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("- Cargo package selection: {packages}"));
        }
        lines.push(format!(
            "- Cargo lock mode: {}",
            self.settings.lock_mode.as_str()
        ));
        lines
    }

    fn resolved_entry(&self) -> (String, ConfigValue) {
        (
            "rust".to_owned(),
            ConfigValue::Object(vec![("cargo".to_owned(), cargo_value(&self.settings))]),
        )
    }

    fn package_id_prefixes(&self) -> &'static [&'static str] {
        &[]
    }

    fn package_id_label(&self) -> &'static str {
        "Cargo package ID"
    }

    fn package_source_label(&self) -> &'static str {
        "Cargo source"
    }

    fn is_package_fallback(&self) -> bool {
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CargoLockMode {
    Locked,
}

impl CargoLockMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Locked => "locked",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CargoSettings {
    manifests: Vec<RepoPath>,
    packages: Vec<String>,
    rules: Vec<PackageRule>,
    lock_mode: CargoLockMode,
    evidence: Vec<CargoEvidence>,
    components: Vec<CargoComponent>,
}

impl CargoSettings {
    pub fn manifests(&self) -> &[RepoPath] {
        &self.manifests
    }

    pub fn packages(&self) -> &[String] {
        &self.packages
    }

    pub fn evidence(&self) -> &[CargoEvidence] {
        &self.evidence
    }

    pub fn components(&self) -> &[CargoComponent] {
        &self.components
    }

    pub fn classify(&self, package: &str, source: &str) -> PackageRuleClassification {
        PackageRule::classify_list(&self.rules, package, source)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CargoEvidenceKind {
    License,
    Notice,
    Materials,
}

impl CargoEvidenceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::License => "license",
            Self::Notice => "notice",
            Self::Materials => "materials",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CargoEvidence {
    package: String,
    version: String,
    source: String,
    repository: String,
    revision: String,
    path: RepoPath,
    url: String,
    kind: CargoEvidenceKind,
}

impl CargoEvidence {
    pub fn package(&self) -> &str {
        &self.package
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn repository(&self) -> &str {
        &self.repository
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn path(&self) -> &RepoPath {
        &self.path
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn kind(&self) -> CargoEvidenceKind {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComponentLayout {
    Independent,
    Centralized,
}

impl ComponentLayout {
    fn as_str(self) -> &'static str {
        match self {
            Self::Independent => "independent",
            Self::Centralized => "centralized",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CargoComponent {
    id: String,
    package: String,
    enabled: bool,
    layout: ComponentLayout,
    materials_directory: Option<RepoPath>,
    license_version: Option<AhclVersion>,
    covered_scope: Option<String>,
    right_holders: Option<Vec<String>>,
    canonical_repository: Option<String>,
    canonical_branch: Option<String>,
    contact: Option<String>,
    adoption_date: Option<ahcl_kit_core::UtcDate>,
    special_authorization_channel: Option<String>,
}

impl CargoComponent {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn package(&self) -> &str {
        &self.package
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn layout(&self) -> ComponentLayout {
        self.layout
    }
    pub fn materials_directory(&self) -> Option<&RepoPath> {
        self.materials_directory.as_ref()
    }
    pub fn license_version(&self) -> Option<AhclVersion> {
        self.license_version
    }
    pub fn covered_scope(&self) -> Option<&str> {
        self.covered_scope.as_deref()
    }
    pub fn right_holders(&self) -> Option<&[String]> {
        self.right_holders.as_deref()
    }
    pub fn canonical_repository(&self) -> Option<&str> {
        self.canonical_repository.as_deref()
    }
    pub fn canonical_branch(&self) -> Option<&str> {
        self.canonical_branch.as_deref()
    }
    pub fn contact(&self) -> Option<&str> {
        self.contact.as_deref()
    }
    pub fn adoption_date(&self) -> Option<ahcl_kit_core::UtcDate> {
        self.adoption_date
    }
    pub fn special_authorization_channel(&self) -> Option<&str> {
        self.special_authorization_channel.as_deref()
    }
}

fn resolve_cargo(document: &ConfigDocument) -> Result<CargoSettings, ConfigError> {
    let manifests = cargo_manifests(document)?;
    let packages = document
        .optional_string_list(Some("rust.cargo"), "packages")?
        .unwrap_or_default();
    let rules = cargo_rules(document)?;
    let lock_mode = cargo_lock_mode(document)?;
    Ok(CargoSettings {
        manifests,
        packages,
        rules,
        lock_mode,
        evidence: resolve_evidence(document)?,
        components: resolve_components(document)?,
    })
}

fn cargo_root_field(key: &str) -> bool {
    matches!(key, "manifests" | "packages" | "rules" | "lock-mode")
}

fn evidence_field_name(key: &str) -> bool {
    matches!(
        key,
        "package" | "version" | "source" | "repository" | "revision" | "path" | "url" | "kind"
    )
}

fn component_field_name(key: &str) -> bool {
    matches!(
        key,
        "package"
            | "enabled"
            | "layout"
            | "materials-directory"
            | "license-version"
            | "covered-scope"
            | "right-holders"
            | "canonical-repository"
            | "canonical-branch"
            | "contact"
            | "adoption-date"
            | "special-authorization-channel"
    )
}

fn cargo_manifests(document: &ConfigDocument) -> Result<Vec<RepoPath>, ConfigError> {
    let manifests = document
        .optional_string_list(Some("rust.cargo"), "manifests")?
        .unwrap_or_else(|| vec!["Cargo.toml".to_owned()])
        .into_iter()
        .map(|path| parse_repo_path("rust.cargo.manifests", &path))
        .collect::<Result<Vec<_>, _>>()?;
    if manifests.is_empty() {
        return invalid("rust.cargo.manifests", "must not be empty when configured");
    }
    reject_duplicate_manifests(&manifests)?;
    Ok(manifests)
}

fn reject_duplicate_manifests(manifests: &[RepoPath]) -> Result<(), ConfigError> {
    let mut portable_manifest_keys = BTreeSet::new();
    for manifest in manifests {
        if !portable_manifest_keys.insert(manifest.as_str().to_lowercase()) {
            return invalid(
                "rust.cargo.manifests",
                &format!("duplicate manifest path: {manifest}"),
            );
        }
    }
    Ok(())
}

fn cargo_rules(document: &ConfigDocument) -> Result<Vec<PackageRule>, ConfigError> {
    document
        .optional_object_list(Some("rust.cargo"), "rules")?
        .unwrap_or_default()
        .into_iter()
        .map(|fields| PackageRule::parse("rust.cargo.rules", false, fields))
        .collect::<Result<Vec<_>, _>>()
}

fn cargo_lock_mode(document: &ConfigDocument) -> Result<CargoLockMode, ConfigError> {
    match document
        .optional_string(Some("rust.cargo"), "lock-mode")?
        .as_deref()
    {
        None | Some("locked") => Ok(CargoLockMode::Locked),
        Some(value) => invalid(
            "rust.cargo.lock-mode",
            &format!("unsupported lock mode: {value}"),
        ),
    }
}

fn resolve_evidence(document: &ConfigDocument) -> Result<Vec<CargoEvidence>, ConfigError> {
    let mut result = Vec::new();
    let mut matching = BTreeMap::<(String, String, String, u8), CargoEvidence>::new();
    for section in document.section_names() {
        if !named_suffix(section, EVIDENCE_PREFIX) {
            continue;
        }
        push_evidence_section(document, section, &mut result, &mut matching)?;
    }
    Ok(result)
}

fn push_evidence_section(
    document: &ConfigDocument,
    section: &str,
    result: &mut Vec<CargoEvidence>,
    matching: &mut BTreeMap<(String, String, String, u8), CargoEvidence>,
) -> Result<(), ConfigError> {
    let fields = document.scalar_fields(section);
    reject_unknown_evidence_fields(section, &fields)?;
    let identity = evidence_identity(section, &fields)?;
    reject_glob_identity(section, &identity)?;
    let located = evidence_location(section, &fields)?;
    remember_evidence(section, identity, located, result, matching)
}

fn reject_unknown_evidence_fields(
    section: &str,
    fields: &BTreeMap<String, ScalarValue>,
) -> Result<(), ConfigError> {
    for key in fields.keys() {
        if !evidence_field_name(key) {
            return invalid(section, &format!("unknown evidence field: {key}"));
        }
    }
    Ok(())
}

fn evidence_identity(
    section: &str,
    fields: &BTreeMap<String, ScalarValue>,
) -> Result<EvidenceIdentity, ConfigError> {
    Ok(EvidenceIdentity {
        package: required_evidence(section, fields, "package")?,
        version: required_evidence(section, fields, "version")?,
        source: required_evidence(section, fields, "source")?,
    })
}

fn required_evidence(
    section: &str,
    fields: &BTreeMap<String, ScalarValue>,
    key: &str,
) -> Result<String, ConfigError> {
    string_field(fields, key)?.ok_or_else(|| ConfigError::InvalidValue {
        path: format!("{section}.{key}"),
        message: "is required".to_owned(),
    })
}

fn reject_glob_identity(section: &str, identity: &EvidenceIdentity) -> Result<(), ConfigError> {
    for (key, value) in [
        ("package", identity.package.as_str()),
        ("version", identity.version.as_str()),
        ("source", identity.source.as_str()),
    ] {
        if identity_has_glob(value) {
            return invalid(
                &format!("{section}.{key}"),
                "must be a non-empty exact value without glob characters",
            );
        }
    }
    Ok(())
}

struct EvidenceIdentity {
    package: String,
    version: String,
    source: String,
}

fn identity_has_glob(value: &str) -> bool {
    value.is_empty() || value.chars().any(evidence_glob_character)
}

fn evidence_glob_character(character: char) -> bool {
    matches!(character, '*' | '?' | '[' | ']')
}

struct EvidenceLocation {
    repository: String,
    revision: String,
    path: RepoPath,
    url: String,
    kind: CargoEvidenceKind,
}

fn evidence_location(
    section: &str,
    fields: &BTreeMap<String, ScalarValue>,
) -> Result<EvidenceLocation, ConfigError> {
    let (repository, revision, path_value, url) = required_evidence_location(section, fields)?;
    // HTTPS and full commit checks happen before the repository path is accepted.
    reject_insecure_evidence_urls(section, &repository, &url)?;
    reject_short_revision(section, &revision)?;
    let path = parse_repo_path(&format!("{section}.path"), &path_value)?;
    let kind = evidence_kind(section, fields)?;
    Ok(EvidenceLocation {
        repository,
        revision,
        path,
        url,
        kind,
    })
}

fn required_evidence_location(
    section: &str,
    fields: &BTreeMap<String, ScalarValue>,
) -> Result<(String, String, String, String), ConfigError> {
    Ok((
        required_evidence(section, fields, "repository")?,
        required_evidence(section, fields, "revision")?,
        required_evidence(section, fields, "path")?,
        required_evidence(section, fields, "url")?,
    ))
}

fn reject_insecure_evidence_urls(
    section: &str,
    repository: &str,
    url: &str,
) -> Result<(), ConfigError> {
    if !is_secure_https_url(repository) || !is_secure_https_url(url) {
        return invalid(section, "repository and url must be absolute HTTPS URLs");
    }
    Ok(())
}

fn reject_short_revision(section: &str, revision: &str) -> Result<(), ConfigError> {
    if is_commit_revision(revision) {
        return Ok(());
    }
    invalid(
        &format!("{section}.revision"),
        "must be a full 40- or 64-character hexadecimal commit",
    )
}

fn evidence_kind(
    section: &str,
    fields: &BTreeMap<String, ScalarValue>,
) -> Result<CargoEvidenceKind, ConfigError> {
    match string_field(fields, "kind")?.as_deref() {
        None | Some("license") => Ok(CargoEvidenceKind::License),
        Some("notice") => Ok(CargoEvidenceKind::Notice),
        Some("materials") => Ok(CargoEvidenceKind::Materials),
        Some(value) => invalid(
            &format!("{section}.kind"),
            &format!("unsupported evidence kind: {value}"),
        ),
    }
}

fn remember_evidence(
    section: &str,
    identity: EvidenceIdentity,
    located: EvidenceLocation,
    result: &mut Vec<CargoEvidence>,
    matching: &mut BTreeMap<(String, String, String, u8), CargoEvidence>,
) -> Result<(), ConfigError> {
    let evidence = CargoEvidence {
        package: identity.package.clone(),
        version: identity.version.clone(),
        source: identity.source.clone(),
        repository: located.repository,
        revision: located.revision,
        path: located.path,
        url: located.url,
        kind: located.kind,
    };
    let key = (
        identity.package,
        identity.version,
        identity.source,
        evidence_kind_key(located.kind),
    );
    if let Some(previous) = matching.insert(key, evidence.clone()) {
        if previous != evidence {
            return invalid(
                section,
                "conflicts with another evidence mapping for the same package, version, and source",
            );
        }
    } else {
        result.push(evidence);
    }
    Ok(())
}

fn evidence_kind_key(kind: CargoEvidenceKind) -> u8 {
    match kind {
        CargoEvidenceKind::License => 0,
        CargoEvidenceKind::Notice => 1,
        CargoEvidenceKind::Materials => 2,
    }
}

fn resolve_components(document: &ConfigDocument) -> Result<Vec<CargoComponent>, ConfigError> {
    let mut result = Vec::new();
    for section in document.section_names() {
        if let Some(component) = component_from_section(document, section)? {
            result.push(component);
        }
    }
    Ok(result)
}

fn component_from_section(
    document: &ConfigDocument,
    section: &str,
) -> Result<Option<CargoComponent>, ConfigError> {
    let Some(id) = section.strip_prefix(COMPONENT_PREFIX) else {
        return Ok(None);
    };
    if !is_config_name(id) {
        return Ok(None);
    }
    parse_component(document, section, id).map(Some)
}

fn parse_component(
    document: &ConfigDocument,
    section: &str,
    id: &str,
) -> Result<CargoComponent, ConfigError> {
    let fields = document.scalar_fields(section);
    let package = component_package(section, &fields)?;
    let materials_directory = component_materials(&fields)?;
    let license_version = component_license(section, &fields)?;
    let covered_scope = component_scope(section, &fields)?;
    let identity = component_identity(document, section, &fields)?;
    let layout = component_layout(section, &fields)?;
    let channels = component_channels(&fields)?;
    Ok(CargoComponent {
        id: id.to_owned(),
        package,
        enabled: channels.enabled,
        layout,
        materials_directory,
        license_version,
        covered_scope,
        right_holders: identity.right_holders,
        canonical_repository: identity.canonical_repository,
        canonical_branch: channels.canonical_branch,
        contact: channels.contact,
        adoption_date: identity.adoption_date,
        special_authorization_channel: channels.special_authorization_channel,
    })
}

fn component_package(
    section: &str,
    fields: &BTreeMap<String, ScalarValue>,
) -> Result<String, ConfigError> {
    let package = string_field(fields, "package")?.ok_or_else(|| ConfigError::InvalidValue {
        path: format!("{section}.package"),
        message: "is required".to_owned(),
    })?;
    if package.is_empty() {
        return invalid(&format!("{section}.package"), "must not be empty");
    }
    Ok(package)
}

fn component_materials(
    fields: &BTreeMap<String, ScalarValue>,
) -> Result<Option<RepoPath>, ConfigError> {
    string_field(fields, "materials-directory")?
        .map(|value| parse_materials_directory(AhclVersion::V1_2, &value))
        .transpose()
}

fn component_license(
    section: &str,
    fields: &BTreeMap<String, ScalarValue>,
) -> Result<Option<AhclVersion>, ConfigError> {
    string_field(fields, "license-version")?
        .map(|value| parse_version(&value, &format!("{section}.license-version")))
        .transpose()
}

fn component_scope(
    section: &str,
    fields: &BTreeMap<String, ScalarValue>,
) -> Result<Option<String>, ConfigError> {
    string_field(fields, "covered-scope")?
        .map(|value| validate_scope(&format!("{section}.covered-scope"), value))
        .transpose()
}

struct ComponentIdentityFields {
    right_holders: Option<Vec<String>>,
    canonical_repository: Option<String>,
    adoption_date: Option<ahcl_kit_core::UtcDate>,
}

fn component_identity(
    document: &ConfigDocument,
    section: &str,
    fields: &BTreeMap<String, ScalarValue>,
) -> Result<ComponentIdentityFields, ConfigError> {
    let right_holders = document.optional_string_list(Some(section), "right-holders")?;
    let canonical_repository = string_field(fields, "canonical-repository")?;
    reject_insecure_repository(section, canonical_repository.as_deref())?;
    Ok(ComponentIdentityFields {
        right_holders,
        canonical_repository,
        adoption_date: component_adoption(section, fields)?,
    })
}

fn reject_insecure_repository(section: &str, value: Option<&str>) -> Result<(), ConfigError> {
    if value.is_some_and(insecure_repository) {
        return invalid(
            &format!("{section}.canonical-repository"),
            "must be an absolute HTTPS URL",
        );
    }
    Ok(())
}

fn insecure_repository(value: &str) -> bool {
    !value.is_empty() && !is_secure_https_url(value)
}

fn component_adoption(
    section: &str,
    fields: &BTreeMap<String, ScalarValue>,
) -> Result<Option<ahcl_kit_core::UtcDate>, ConfigError> {
    string_field(fields, "adoption-date")?
        .filter(|value| !value.is_empty())
        .map(|value| parse_date_at(&value, &format!("{section}.adoption-date")))
        .transpose()
}

fn component_layout(
    section: &str,
    fields: &BTreeMap<String, ScalarValue>,
) -> Result<ComponentLayout, ConfigError> {
    match string_field(fields, "layout")?.as_deref() {
        None | Some("independent") => Ok(ComponentLayout::Independent),
        Some("centralized") => Ok(ComponentLayout::Centralized),
        Some(value) => invalid(
            &format!("{section}.layout"),
            &format!("unsupported component layout: {value}"),
        ),
    }
}

struct ComponentChannels {
    enabled: bool,
    canonical_branch: Option<String>,
    contact: Option<String>,
    special_authorization_channel: Option<String>,
}

fn component_channels(
    fields: &BTreeMap<String, ScalarValue>,
) -> Result<ComponentChannels, ConfigError> {
    Ok(ComponentChannels {
        enabled: boolean_field(fields, "enabled")?.unwrap_or(true),
        canonical_branch: string_field(fields, "canonical-branch")?,
        contact: string_field(fields, "contact")?,
        special_authorization_channel: string_field(fields, "special-authorization-channel")?,
    })
}

fn cargo_value(settings: &CargoSettings) -> ConfigValue {
    ConfigValue::Object(vec![
        (
            "manifests".to_owned(),
            ConfigValue::strings(settings.manifests.iter().map(|path| path.as_str())),
        ),
        (
            "packages".to_owned(),
            ConfigValue::strings(settings.packages.iter().map(String::as_str)),
        ),
        (
            "rules".to_owned(),
            ConfigValue::Array(
                settings
                    .rules
                    .iter()
                    .map(PackageRule::resolved_value)
                    .collect(),
            ),
        ),
        (
            "evidence".to_owned(),
            ConfigValue::Array(settings.evidence.iter().map(evidence_value).collect()),
        ),
        (
            "components".to_owned(),
            ConfigValue::Array(settings.components.iter().map(component_value).collect()),
        ),
        (
            "lock_mode".to_owned(),
            ConfigValue::String(settings.lock_mode.as_str().to_owned()),
        ),
    ])
}

fn evidence_value(evidence: &CargoEvidence) -> ConfigValue {
    ConfigValue::Object(vec![
        (
            "package".to_owned(),
            ConfigValue::String(evidence.package.clone()),
        ),
        (
            "version".to_owned(),
            ConfigValue::String(evidence.version.clone()),
        ),
        (
            "source".to_owned(),
            ConfigValue::String(evidence.source.clone()),
        ),
        (
            "repository".to_owned(),
            ConfigValue::String(evidence.repository.clone()),
        ),
        (
            "revision".to_owned(),
            ConfigValue::String(evidence.revision.clone()),
        ),
        (
            "path".to_owned(),
            ConfigValue::String(evidence.path.as_str().to_owned()),
        ),
        ("url".to_owned(), ConfigValue::String(evidence.url.clone())),
        (
            "kind".to_owned(),
            ConfigValue::String(evidence.kind.as_str().to_owned()),
        ),
    ])
}

fn component_value(component: &CargoComponent) -> ConfigValue {
    ConfigValue::Object(vec![
        ("id".to_owned(), ConfigValue::String(component.id.clone())),
        (
            "package".to_owned(),
            ConfigValue::String(component.package.clone()),
        ),
        ("enabled".to_owned(), ConfigValue::Bool(component.enabled)),
        (
            "layout".to_owned(),
            ConfigValue::String(component.layout.as_str().to_owned()),
        ),
        (
            "materials_directory".to_owned(),
            component
                .materials_directory
                .as_ref()
                .map(|path| ConfigValue::String(path.as_str().to_owned()))
                .unwrap_or(ConfigValue::Null),
        ),
        (
            "license_version".to_owned(),
            component
                .license_version
                .map(|version| ConfigValue::String(version.as_str().to_owned()))
                .unwrap_or(ConfigValue::Null),
        ),
        (
            "covered_scope".to_owned(),
            component
                .covered_scope
                .clone()
                .map(ConfigValue::String)
                .unwrap_or(ConfigValue::Null),
        ),
    ])
}

fn string_field(
    fields: &BTreeMap<String, ScalarValue>,
    key: &str,
) -> Result<Option<String>, ConfigError> {
    match fields.get(key) {
        None => Ok(None),
        Some(ScalarValue::String(value)) => Ok(Some(value.clone())),
        Some(_) => invalid("rust.cargo.rules", &format!("{key} must be a string")),
    }
}

fn boolean_field(
    fields: &BTreeMap<String, ScalarValue>,
    key: &str,
) -> Result<Option<bool>, ConfigError> {
    match fields.get(key) {
        None => Ok(None),
        Some(ScalarValue::Boolean(value)) => Ok(Some(*value)),
        Some(_) => invalid(key, "must be a boolean"),
    }
}

fn named_suffix(section: &str, prefix: &str) -> bool {
    section.strip_prefix(prefix).is_some_and(is_config_name)
}

fn invalid<T>(path: &str, message: &str) -> Result<T, ConfigError> {
    Err(ConfigError::InvalidValue {
        path: path.to_owned(),
        message: message.to_owned(),
    })
}
