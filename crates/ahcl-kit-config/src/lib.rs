// crates/ahcl-kit-config/src/lib.rs - Public API for configuration parsing and resolution.
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

//! Strict parsing and resolution for `.ahclkitconfigs` files.

mod ast;
mod binding;
mod host;
mod parser;
mod schema;
mod skeleton;

pub use ast::ScalarValue;
pub use binding::{ConfigValue, LanguageBinding, LanguageContributor, inline_text, is_config_name};
pub use host::{ComponentResolution, HostFailure, LanguageHost, LanguageInstallation};
pub use parser::{ConfigDocument, ConfigError};
pub use schema::{
    AhclVersion, ComponentIdentity, ConfigLimits, EffectiveConfig, GenerationSettings,
    LATEST_SCHEMA, LicenseSettings, PackageRule, PackageRuleClassification, ProjectSettings,
    is_absolute_https_url, is_commit_revision, is_secure_https_url, parse_date_at,
    parse_materials_directory, parse_repo_path, parse_version, validate_scope,
};
pub use skeleton::{ConfigSkeleton, ProjectIdentity};
