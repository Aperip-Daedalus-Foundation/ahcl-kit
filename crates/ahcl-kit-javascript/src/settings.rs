// crates/ahcl-kit-javascript/src/settings.rs - JavaScript configuration owned by its adapter.
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
    ConfigDocument, ConfigError, ConfigValue, EffectiveConfig, LanguageBinding,
    LanguageContributor, PackageRule, PackageRuleClassification, inline_text, parse_repo_path,
};
use ahcl_kit_core::RepoPath;
use std::any::Any;
use std::collections::BTreeSet;

pub static CONTRIBUTOR: JavascriptContributor = JavascriptContributor;

#[derive(Clone, Copy, Debug)]
pub struct JavascriptContributor;

impl LanguageContributor for JavascriptContributor {
    fn language_id(&self) -> &'static str {
        "javascript"
    }

    fn display_name(&self) -> &'static str {
        "JavaScript"
    }

    fn skeleton_hint(&self) -> &'static str {
        "# To enable JavaScript package managers, use `javascript` and:\n# [javascript]\n# manifests:\n#   - \"package.json\"\n# managers:\n#   - \"npm\"\n# packages = []\n# rules = []\n#\n# `managers` accepts npm, pnpm, yarn, and bun. Omit it to select\n# the single lockfile beside each manifest.\n"
    }

    fn owns_section(&self, section: &str) -> bool {
        section == "javascript"
    }

    fn known_field(&self, section: &str, key: &str) -> bool {
        section == "javascript" && matches!(key, "manifests" | "managers" | "packages" | "rules")
    }

    fn load(
        &self,
        document: &ConfigDocument,
        enabled: bool,
    ) -> Result<Box<dyn LanguageBinding>, ConfigError> {
        Ok(Box::new(JavascriptBinding {
            settings: resolve_javascript(document)?,
            enabled,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JavascriptBinding {
    settings: JavascriptSettings,
    enabled: bool,
}

impl JavascriptBinding {
    pub fn settings(&self) -> &JavascriptSettings {
        &self.settings
    }

    pub fn from_config(config: &EffectiveConfig) -> Option<&Self> {
        config
            .bindings()
            .iter()
            .find_map(|binding| binding.as_any().downcast_ref::<Self>())
    }
}

impl LanguageBinding for JavascriptBinding {
    fn language_id(&self) -> &'static str {
        "javascript"
    }

    fn display_name(&self) -> &'static str {
        "JavaScript"
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
        if !self.enabled {
            return Vec::new();
        }
        let mut lines = Vec::new();
        lines.push(format!(
            "- JavaScript manifests: {}",
            joined_paths(&self.settings.manifests)
        ));
        if self.settings.managers.is_empty() {
            lines.push(
                "- JavaScript package managers: Detect the single lockfile beside each manifest."
                    .to_owned(),
            );
        } else {
            let managers = self
                .settings
                .managers
                .iter()
                .map(|manager| manager.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("- JavaScript package managers: {managers}"));
        }
        if self.settings.packages.is_empty() {
            lines.push("- JavaScript package selection: All workspace roots.".to_owned());
        } else {
            let packages = self
                .settings
                .packages
                .iter()
                .map(|package| format!("`{}`", inline_text(package)))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("- JavaScript package selection: {packages}"));
        }
        lines
    }

    fn resolved_entry(&self) -> (String, ConfigValue) {
        (
            "javascript".to_owned(),
            ConfigValue::Object(vec![
                (
                    "manifests".to_owned(),
                    ConfigValue::strings(self.settings.manifests.iter().map(|path| path.as_str())),
                ),
                (
                    "managers".to_owned(),
                    ConfigValue::strings(
                        self.settings
                            .managers
                            .iter()
                            .map(|manager| manager.as_str()),
                    ),
                ),
                (
                    "packages".to_owned(),
                    ConfigValue::strings(self.settings.packages.iter().map(String::as_str)),
                ),
                (
                    "rules".to_owned(),
                    ConfigValue::Array(
                        self.settings
                            .rules
                            .iter()
                            .map(PackageRule::resolved_value)
                            .collect(),
                    ),
                ),
            ]),
        )
    }

    fn package_id_prefixes(&self) -> &'static [&'static str] {
        &["npm:", "pnpm:", "yarn:", "bun:"]
    }

    fn package_id_label(&self) -> &'static str {
        "Package ID"
    }

    fn package_source_label(&self) -> &'static str {
        "Source"
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum JsPackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl JsPackageManager {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JavascriptSettings {
    manifests: Vec<RepoPath>,
    managers: Vec<JsPackageManager>,
    packages: Vec<String>,
    rules: Vec<PackageRule>,
}

impl JavascriptSettings {
    pub fn from_manifests(manifests: &[RepoPath]) -> Self {
        let manifests = if manifests.is_empty() {
            vec![RepoPath::parse("package.json").expect("package.json is a repository path")]
        } else {
            manifests.to_vec()
        };
        Self {
            manifests,
            managers: Vec::new(),
            packages: Vec::new(),
            rules: Vec::new(),
        }
    }

    pub fn manifests(&self) -> &[RepoPath] {
        &self.manifests
    }

    pub fn managers(&self) -> &[JsPackageManager] {
        &self.managers
    }

    pub fn packages(&self) -> &[String] {
        &self.packages
    }

    pub fn classify(&self, package: &str, source: &str) -> PackageRuleClassification {
        PackageRule::classify_list(&self.rules, package, source)
    }
}

fn resolve_javascript(document: &ConfigDocument) -> Result<JavascriptSettings, ConfigError> {
    let manifests = javascript_manifests(document)?;
    let managers = javascript_managers(document)?;
    let packages = document
        .optional_string_list(Some("javascript"), "packages")?
        .unwrap_or_default();
    let rules = javascript_rules(document)?;
    Ok(JavascriptSettings {
        manifests,
        managers,
        packages,
        rules,
    })
}

fn javascript_manifests(document: &ConfigDocument) -> Result<Vec<RepoPath>, ConfigError> {
    let manifests = document
        .optional_string_list(Some("javascript"), "manifests")?
        .unwrap_or_else(|| vec!["package.json".to_owned()])
        .into_iter()
        .map(|path| parse_repo_path("javascript.manifests", &path))
        .collect::<Result<Vec<_>, _>>()?;
    if manifests.is_empty() {
        return invalid("javascript.manifests", "must not be empty when configured");
    }
    reject_duplicate_manifests(&manifests)?;
    Ok(manifests)
}

fn reject_duplicate_manifests(manifests: &[RepoPath]) -> Result<(), ConfigError> {
    let mut portable_manifest_keys = BTreeSet::new();
    for manifest in manifests {
        if !portable_manifest_keys.insert(manifest.as_str().to_lowercase()) {
            return invalid(
                "javascript.manifests",
                &format!("duplicate manifest path: {manifest}"),
            );
        }
    }
    Ok(())
}

fn javascript_managers(document: &ConfigDocument) -> Result<Vec<JsPackageManager>, ConfigError> {
    let managers = match document.optional_string_list(Some("javascript"), "managers")? {
        None => Vec::new(),
        Some(values) => parse_managers(values)?,
    };
    if managers.iter().collect::<BTreeSet<_>>().len() != managers.len() {
        return invalid("javascript.managers", "duplicate package manager");
    }
    Ok(managers)
}

fn parse_managers(values: Vec<String>) -> Result<Vec<JsPackageManager>, ConfigError> {
    if values.is_empty() {
        return invalid("javascript.managers", "must not be empty when configured");
    }
    values
        .into_iter()
        .map(|value| parse_manager(&value))
        .collect()
}

fn javascript_rules(document: &ConfigDocument) -> Result<Vec<PackageRule>, ConfigError> {
    document
        .optional_object_list(Some("javascript"), "rules")?
        .unwrap_or_default()
        .into_iter()
        .map(|fields| PackageRule::parse("javascript.rules", true, fields))
        .collect()
}

fn parse_manager(value: &str) -> Result<JsPackageManager, ConfigError> {
    match value {
        "npm" => Ok(JsPackageManager::Npm),
        "pnpm" => Ok(JsPackageManager::Pnpm),
        "yarn" => Ok(JsPackageManager::Yarn),
        "bun" => Ok(JsPackageManager::Bun),
        _ => invalid(
            "javascript.managers",
            &format!("unsupported package manager: {value}"),
        ),
    }
}

fn joined_paths(paths: &[RepoPath]) -> String {
    paths
        .iter()
        .map(|path| format!("`{}`", path.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn invalid<T>(path: &str, message: &str) -> Result<T, ConfigError> {
    Err(ConfigError::InvalidValue {
        path: path.to_owned(),
        message: message.to_owned(),
    })
}
