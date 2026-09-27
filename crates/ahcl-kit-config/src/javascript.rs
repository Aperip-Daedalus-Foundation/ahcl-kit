// crates/ahcl-kit-config/src/javascript.rs - JavaScript package-manager configuration.
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

use crate::ast::DocumentAst;
use crate::parser::ConfigError;
use crate::schema::{
    CargoRule, CargoRuleClassification, invalid, optional_object_list, optional_string_list,
    parse_repo_path, resolve_package_rule,
};
use ahcl_kit_core::RepoPath;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
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
    rules: Vec<CargoRule>,
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

    pub fn rules(&self) -> &[CargoRule] {
        &self.rules
    }

    pub fn classify(&self, package: &str, source: &str) -> CargoRuleClassification {
        self.rules
            .iter()
            .rev()
            .find(|rule| rule.applies(package, source))
            .map(CargoRule::classification)
            .unwrap_or(CargoRuleClassification::ThirdParty)
    }
}

pub(crate) fn resolve_javascript(ast: &DocumentAst) -> Result<JavascriptSettings, ConfigError> {
    let manifests = optional_string_list(ast, Some("javascript"), "manifests")?
        .unwrap_or_else(|| vec!["package.json".to_owned()])
        .into_iter()
        .map(|path| parse_repo_path("javascript.manifests", &path))
        .collect::<Result<Vec<_>, _>>()?;
    if manifests.is_empty() {
        return invalid(
            "javascript.manifests",
            "must not be empty when configured".to_owned(),
        );
    }
    let mut portable_manifest_keys = BTreeSet::new();
    for manifest in &manifests {
        if !portable_manifest_keys.insert(manifest.as_str().to_lowercase()) {
            return invalid(
                "javascript.manifests",
                format!("duplicate manifest path: {manifest}"),
            );
        }
    }
    let managers = match optional_string_list(ast, Some("javascript"), "managers")? {
        None => Vec::new(),
        Some(values) if values.is_empty() => {
            return invalid(
                "javascript.managers",
                "must not be empty when configured".to_owned(),
            );
        }
        Some(values) => values
            .into_iter()
            .map(|value| parse_manager(&value))
            .collect::<Result<Vec<_>, _>>()?,
    };
    if managers.iter().collect::<BTreeSet<_>>().len() != managers.len() {
        return invalid(
            "javascript.managers",
            "duplicate package manager".to_owned(),
        );
    }
    let packages = optional_string_list(ast, Some("javascript"), "packages")?.unwrap_or_default();
    let rules = optional_object_list(ast, Some("javascript"), "rules")?
        .unwrap_or_default()
        .into_iter()
        .map(|fields| resolve_package_rule("javascript.rules", fields))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(JavascriptSettings {
        manifests,
        managers,
        packages,
        rules,
    })
}

fn parse_manager(value: &str) -> Result<JsPackageManager, ConfigError> {
    match value {
        "npm" => Ok(JsPackageManager::Npm),
        "pnpm" => Ok(JsPackageManager::Pnpm),
        "yarn" => Ok(JsPackageManager::Yarn),
        "bun" => Ok(JsPackageManager::Bun),
        _ => invalid(
            "javascript.managers",
            format!("unsupported package manager: {value}"),
        ),
    }
}
