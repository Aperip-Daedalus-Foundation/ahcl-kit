// crates/ahcl-kit-config/src/skeleton.rs - Starter configuration rendering.
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

use crate::binding::LanguageContributor;
use crate::schema::LATEST_SCHEMA;

/// Renders a complete, deterministic starter configuration with no adapters enabled.
pub struct ConfigSkeleton;

/// Identity supplied to project initialization when no configuration exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectIdentity {
    name: String,
    canonical_repository: String,
    right_holders: Vec<String>,
}

impl ProjectIdentity {
    pub fn new(
        name: impl Into<String>,
        canonical_repository: impl Into<String>,
        right_holders: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            canonical_repository: canonical_repository.into(),
            right_holders,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn canonical_repository(&self) -> &str {
        &self.canonical_repository
    }

    pub fn right_holders(&self) -> &[String] {
        &self.right_holders
    }
}

impl ConfigSkeleton {
    pub fn render(contributors: &[&'static dyn LanguageContributor]) -> String {
        let mut rendered = format!(
            "# AHCL Kit project configuration.\n\
             schema = {LATEST_SCHEMA}\n\
             materials-directory = \".ahcl\"\n\
             languages = []\n\
             \n\
             [project]\n\
             name = \"\"\n\
             canonical-repository = \"\"\n\
             canonical-branch = \"master\"\n\
             right-holders = []\n\
             contact = \"\"\n\
             adoption-date = \"\"\n\
             \n\
             [license]\n\
             version = \"1.2\"\n\
             special-authorization-channel = \"\"\n\
             \n\
             [generation]\n\
             strict-license-files = true\n",
        );
        for contributor in contributors {
            rendered.push_str(contributor.skeleton_hint());
        }
        rendered
    }

    /// Renders a complete no-language configuration from project-init flags.
    pub fn render_populated(identity: &ProjectIdentity) -> String {
        let mut rendered = format!(
            "schema = {LATEST_SCHEMA}\nmaterials-directory = \".ahcl\"\nlanguages = []\n\n[project]\n",
        );
        rendered.push_str("name = ");
        rendered.push_str(&quoted(identity.name()));
        rendered.push_str("\ncanonical-repository = ");
        rendered.push_str(&quoted(identity.canonical_repository()));
        rendered.push_str("\ncanonical-branch = \"master\"\n");
        if identity.right_holders().is_empty() {
            rendered.push_str("right-holders = []\n");
        } else {
            rendered.push_str("right-holders:");
            for holder in identity.right_holders() {
                rendered.push_str("\n  - ");
                rendered.push_str(&quoted(holder));
            }
            rendered.push('\n');
        }
        rendered.push_str(
            "contact = \"\"\nadoption-date = \"\"\n\n[license]\nversion = \"1.2\"\nspecial-authorization-channel = \"\"\n\n[generation]\nstrict-license-files = true\n",
        );
        rendered
    }
}

fn quoted(value: &str) -> String {
    let mut rendered = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => rendered.push_str("\\\\"),
            '\"' => rendered.push_str("\\\""),
            '\n' => rendered.push_str("\\n"),
            '\r' => rendered.push_str("\\r"),
            '\t' => rendered.push_str("\\t"),
            _ => rendered.push(character),
        }
    }
    rendered.push('\"');
    rendered
}
