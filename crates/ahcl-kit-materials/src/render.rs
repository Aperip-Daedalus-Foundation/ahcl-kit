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
        AhclVersion::V1_2 => {
            let scope = covered_scope(config);
            let key = scope_key(config);
            format!(
                "----- BEGIN AHCL NOTICE -----\n\n\
                 <!-- AHCL KIT MANAGED SCOPE: {key} -->\n\
                 This license notice applies to:\n\
                 {project}\n\n\
                 AHCL-covered portions:\n\
                 {scope}\n\n\
                 The portions identified above are licensed under version 1.2 of the\n\
                 Aperip Heimdall Commons License (AHCL 1.2), subject to its provisions\n\
                 concerning migration to later official versions.\n\n\
                 Official AHCL text, announcements, and public notices:\n\
                 https://ahcl.aperip.com\n\n\
                 AHCL Materials Directory (relative to the directory containing this LICENSE):\n\
                 {directory}/\n\n\
                 Official or recognized AHCL 1.2 copy:\n\
                 {directory}/{}\n\n\
                 ----- END AHCL NOTICE -----\n",
                license.source_filename,
                project = inline(config.project().name()),
            )
        }
    };

    let channel = inline(config.license().special_authorization_channel());
    if !channel.is_empty() {
        let mut section = String::from("\nChannels for Non-AHCL Special Authorizations:\n");
        section.push_str(&channel);
        section.push_str(
            "\n\nThe channel above is solely for applying for or obtaining a separate\n\
             Special Authorization. Channel information does not itself constitute a Special\n\
             Authorization, does not modify AHCL ",
        );
        section.push_str(version);
        section.push_str(
            ", and does not waive or reduce any AHCL\n\
             obligation not expressly covered by a valid written Special Authorization.\n",
        );
        if config.license().version() == AhclVersion::V1_2 {
            let marker = "\n----- END AHCL NOTICE -----\n";
            if let Some(index) = rendered.rfind(marker) {
                rendered.insert_str(index, &section);
            } else {
                rendered.push_str(&section);
            }
        } else {
            rendered.push_str(&section);
        }
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
    if config.license().version() == AhclVersion::V1_2 {
        rendered.push_str("- Covered scope: ");
        rendered.push_str(&covered_scope(config));
        rendered.push_str("\n- Applicable LICENSE: `LICENSE`\n");
    }
    rendered.push_str("- AHCL origin: <https://ahcl.aperip.com>\n- Applicable version: AHCL ");
    rendered.push_str(config.license().version().as_str());
    rendered.push_str("\n- Continuous AHCL Licensing Segment beginning: ");
    rendered.push_str(&adoption_date.to_string());
    if config.license().version() == AhclVersion::V1_2 {
        rendered.push_str("\n- Effective Date: ");
        rendered.push_str(&adoption_date.to_string());
    }
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
    rendered.push('`');
    if config.license().version() == AhclVersion::V1_2 {
        rendered.push_str("\n- Covered scope: ");
        rendered.push_str(&covered_scope(config));
    }
    rendered.push_str(
        "\n\nPublic Source Code, Complete Modification History, build materials, and release mappings are available from the canonical repository above.\n",
    );
    rendered
}

pub(crate) fn version_adoption(config: &EffectiveConfig, adoption_date: UtcDate) -> String {
    if config.license().version() != AhclVersion::V1_2 {
        return format!(
            "# AHCL Version Adoption\n\n\
             - Initial AHCL adoption date: {adoption_date}\n\
             - Applicable version: AHCL {}\n\n\
             ## Version Adoption Events\n\n\
             None.\n",
            config.license().version().as_str()
        );
    }
    let key = scope_key(config);
    format!(
        "# AHCL Version Adoption\n\n\
         - Covered scope: {}\n\
         - Initial AHCL adoption date: {adoption_date}\n\
         - Applicable version: AHCL {}\n\n\
         ## Version Adoption Events\n\n\
         <!-- BEGIN AHCL KIT MANAGED SCOPE: {key} -->\n\
         - Version adoption event: AHCL {} for {}\n\
         - Effective Date: {adoption_date}\n\
         <!-- END AHCL KIT MANAGED SCOPE: {key} -->\n",
        covered_scope(config),
        config.license().version().as_str(),
        config.license().version().as_str(),
        covered_scope(config),
    )
}

pub(crate) fn managed_block(config: &EffectiveConfig, body: &str) -> String {
    let key = scope_key(config);
    format!(
        "<!-- BEGIN AHCL KIT MANAGED SCOPE: {key} -->\n{body}<!-- END AHCL KIT MANAGED SCOPE: {key} -->\n"
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

pub(crate) fn covered_scope(config: &EffectiveConfig) -> String {
    let scope = config.license().covered_scope().trim();
    if scope.is_empty() {
        "the project as a whole".to_owned()
    } else {
        inline(scope)
    }
}

pub(crate) fn scope_key(config: &EffectiveConfig) -> String {
    let source = config.license().covered_scope().trim();
    if source.is_empty() {
        return "project".to_owned();
    }
    let mut key = String::new();
    let mut separator = false;
    for character in source.chars() {
        if character.is_ascii_alphanumeric() {
            key.push(character.to_ascii_lowercase());
            separator = false;
        } else if !key.is_empty() {
            separator = true;
        }
        if separator && !key.ends_with('-') {
            key.push('-');
        }
    }
    let key = key.trim_matches('-').to_owned();
    if key.is_empty() {
        "project".to_owned()
    } else {
        key
    }
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
