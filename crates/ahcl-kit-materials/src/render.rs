// crates/ahcl-kit-materials/src/render.rs - AHCL project material rendering.
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

use crate::LayoutPolicy;
use ahcl_kit_config::{AhclVersion, EffectiveConfig};
use ahcl_kit_core::UtcDate;
use ahcl_kit_license::VerifiedLicense;

pub(crate) fn root_license(
    config: &EffectiveConfig,
    layout: &LayoutPolicy,
    license: &VerifiedLicense,
) -> String {
    let version = config.license().version().as_str();
    let directory = layout.materials_directory().as_str();
    let mut rendered = match config.license().version() {
        AhclVersion::V1_0 => format!(
            "This project is licensed under version 1.0 of the Aperip Heimdall Commons\n\
             License (AHCL 1.0).\n\n\
             Official AHCL English text, announcements, and public notices:\n\
             https://ahcl.aperip.com\n\n\
             Verbatim AHCL 1.0 copy in this repository:\n\
             {directory}/{}\n",
            license.source_filename
        ),
        AhclVersion::V1_1 => format!(
            "This project is licensed under version 1.1 of the Aperip Heimdall Commons\n\
             License (AHCL 1.1).\n\n\
             Official AHCL text, announcements, and public notices:\n\
             https://ahcl.aperip.com\n\n\
             AHCL Materials Directory:\n\
             {directory}/\n\n\
             Official or recognized AHCL 1.1 copy in this repository:\n\
             {directory}/{}\n",
            license.source_filename
        ),
    };

    let channel = inline(config.license().special_authorization_channel());
    if !channel.is_empty() {
        rendered.push_str("\nChannels for Non-AHCL Special Authorizations:\n");
        rendered.push_str(&channel);
        rendered.push_str(
            "\n\nThe channel above is solely for applying for or obtaining a separate\n\
             Special Authorization. Channel information does not itself constitute a Special\n\
             Authorization, does not modify AHCL ",
        );
        rendered.push_str(version);
        rendered.push_str(
            ", and does not waive or reduce any AHCL\n\
             obligation not expressly covered by a valid written Special Authorization.\n",
        );
    }
    rendered
}

pub(crate) fn project_notice(
    config: &EffectiveConfig,
    layout: &LayoutPolicy,
    adoption_date: UtcDate,
) -> String {
    let project = config.project();
    let mut rendered = String::from("# AHCL Project Notice\n\n- Project: ");
    rendered.push_str(&inline(project.name()));
    rendered.push_str("\n- Canonical repository: <");
    rendered.push_str(project.canonical_repository());
    rendered.push_str(">\n- Right holders:\n");
    if project.right_holders().is_empty() {
        rendered.push_str("  - None.\n");
    } else {
        for holder in project.right_holders() {
            rendered.push_str("  - ");
            rendered.push_str(&inline(holder));
            rendered.push('\n');
        }
    }
    rendered.push_str("- AHCL origin: <https://ahcl.aperip.com>\n- Applicable version: AHCL ");
    rendered.push_str(config.license().version().as_str());
    rendered.push_str("\n- Continuous AHCL Licensing Segment beginning: ");
    rendered.push_str(&adoption_date.to_string());
    rendered.push_str("\n- Version adoption records: `");
    rendered.push_str(layout.materials_directory().as_str());
    rendered.push_str("/AHCL-VERSION-ADOPTION.md`\n");
    rendered
}

pub(crate) fn source(config: &EffectiveConfig) -> String {
    let project = config.project();
    let mut rendered = String::from("# AHCL Source and History\n\n- Canonical repository: <");
    rendered.push_str(project.canonical_repository());
    rendered.push_str(">\n- Canonical branch: `");
    rendered.push_str(&inline(project.canonical_branch()));
    rendered.push_str(
        "`\n\nPublic Source Code, Complete Modification History, build materials, and release mappings are available from the canonical repository above.\n",
    );
    rendered
}

pub(crate) fn version_adoption(config: &EffectiveConfig, adoption_date: UtcDate) -> String {
    format!(
        "# AHCL Version Adoption\n\n\
         - Initial AHCL adoption date: {adoption_date}\n\
         - Applicable version: AHCL {}\n\n\
         ## Version Adoption Events\n\n\
         None.\n",
        config.license().version().as_str()
    )
}

pub(crate) fn empty_dependencies(config: &EffectiveConfig) -> String {
    let adapters = if config.languages().is_empty() {
        "None.".to_owned()
    } else {
        config
            .languages()
            .iter()
            .map(|language| match language {
                ahcl_kit_config::Language::Rust => "Rust",
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "# AHCL Dependencies and Referenced Materials\n\n\
         Generated by AHCL Kit for {}.\n\n\
         Configuration summary:\n\
         - AHCL version: {}\n\
         - Materials directory: `{}`\n\
         - Ecosystem adapters: {adapters}\n\n\
         ## Dependencies and Referenced Materials\n\n\
         None.\n",
        inline(config.project().name()),
        config.license().version().as_str(),
        layout_directory(config),
    )
}

pub(crate) fn special_authorizations(config: &EffectiveConfig) -> String {
    let channel = inline(config.license().special_authorization_channel());
    if channel.is_empty() {
        return "No public information.\n".to_owned();
    }
    format!("# AHCL Special Authorizations\n\nPublic channel: {channel}\n")
}

fn layout_directory(config: &EffectiveConfig) -> &str {
    config.materials_directory().as_str()
}

fn inline(value: &str) -> String {
    let mut rendered = String::with_capacity(value.len());
    let mut previous_was_space = false;
    for character in value.chars() {
        let character = if character.is_control() {
            ' '
        } else {
            character
        };
        if character == ' ' {
            if !previous_was_space {
                rendered.push(' ');
            }
            previous_was_space = true;
        } else {
            if character == '`' {
                rendered.push('\\');
            }
            rendered.push(character);
            previous_was_space = false;
        }
    }
    rendered.trim().to_owned()
}
