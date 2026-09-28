// crates/ahcl-kit-config/src/binding.rs - Language-neutral adapter configuration contracts.
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

use crate::{ConfigDocument, ConfigError};
use ahcl_kit_core::ResolvedPackage;
use std::any::Any;

/// One installed language adapter's configuration schema and loader.
///
/// The composition root supplies these. The configuration crate does not name
/// concrete languages.
pub trait LanguageContributor: Send + Sync {
    fn language_id(&self) -> &'static str;

    fn display_name(&self) -> &'static str;

    fn skeleton_hint(&self) -> &'static str;

    fn owns_section(&self, section: &str) -> bool;

    fn known_field(&self, section: &str, key: &str) -> bool;

    fn load(
        &self,
        document: &ConfigDocument,
        enabled: bool,
    ) -> Result<Box<dyn LanguageBinding>, ConfigError>;
}

/// Resolved settings for one language, stored on an effective configuration.
pub trait LanguageBinding: Send + Sync {
    fn language_id(&self) -> &'static str;

    fn display_name(&self) -> &'static str;

    fn is_enabled(&self) -> bool;

    fn clone_box(&self) -> Box<dyn LanguageBinding>;

    fn as_any(&self) -> &dyn Any;

    fn summary_lines(&self) -> Vec<String>;

    fn resolved_entry(&self) -> (String, ConfigValue);

    fn package_id_prefixes(&self) -> &'static [&'static str];

    fn package_id_label(&self) -> &'static str;

    fn package_source_label(&self) -> &'static str;

    /// Whether unlabeled packages use this binding's field labels.
    ///
    /// At most one installed binding may say yes. Prefix-owning bindings win
    /// before this fallback is considered.
    fn is_package_fallback(&self) -> bool {
        false
    }

    fn owns_package(&self, package: &ResolvedPackage) -> bool {
        let prefixes = self.package_id_prefixes();
        if prefixes.is_empty() {
            return false;
        }
        prefixes.iter().any(|prefix| package.id.starts_with(prefix))
    }
}

/// Ordered configuration value used by resolved-config output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigValue {
    Null,
    Bool(bool),
    Integer(u64),
    String(String),
    Array(Vec<ConfigValue>),
    Object(Vec<(String, ConfigValue)>),
}

impl ConfigValue {
    pub fn strings<'a>(values: impl IntoIterator<Item = &'a str>) -> Self {
        Self::Array(
            values
                .into_iter()
                .map(|value| Self::String(value.to_owned()))
                .collect(),
        )
    }
}

pub fn inline_text(value: &str) -> String {
    let mut rendered = String::new();
    for character in value.chars() {
        if character.is_control() {
            rendered.push(' ');
        } else {
            if character == '`' {
                rendered.push('\\');
            }
            rendered.push(character);
        }
    }
    rendered.trim().to_owned()
}

pub fn is_config_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
