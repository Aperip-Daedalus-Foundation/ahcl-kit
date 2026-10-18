// crates/ahcl-kit-config/src/schema_component.rs - Component identity and history narrowing.
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

use super::{AhclVersion, EffectiveConfig, LicenseSettings, ProjectSettings, invalid};
use crate::history::matching_scope;
use crate::{ConfigError, HistoryOverrides, LicensingHistory};
use ahcl_kit_core::{RepoPath, UtcDate};

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

impl EffectiveConfig {
    pub fn for_component_identity(&self, identity: ComponentIdentity) -> Result<Self, ConfigError> {
        self.for_component_identity_with_history(identity, None)
    }

    /// Narrows identity first, then applies and validates local history settings.
    /// Signed parent snapshots retain their original scope and identity.
    pub fn for_component_identity_with_history(
        &self,
        identity: ComponentIdentity,
        history: Option<&HistoryOverrides>,
    ) -> Result<Self, ConfigError> {
        let version = identity.license_version.unwrap_or(self.license.version);
        let materials_directory = identity
            .materials_directory
            .clone()
            .unwrap_or_else(|| self.materials_directory.clone());
        validate_centralized(self, &identity, version, &materials_directory)?;
        let right_holders = component_right_holders(&identity, &self.project.right_holders)?;
        let project = narrowed_project(&self.project, &identity, right_holders);
        let license = narrowed_license(&self.license, &identity, &project, history)?;
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
        if matching_scope(&cloned.license.covered_scope) != matching_scope(covered_scope) {
            cloned.license.history = LicensingHistory::initial(cloned.project.adoption_date);
        }
        cloned.license.covered_scope = covered_scope.to_owned();
        cloned
    }
}

fn validate_centralized(
    parent: &EffectiveConfig,
    identity: &ComponentIdentity,
    version: AhclVersion,
    directory: &RepoPath,
) -> Result<(), ConfigError> {
    if identity.centralized
        && (parent.license.version != AhclVersion::V1_2
            || version != AhclVersion::V1_2
            || *directory != parent.materials_directory)
    {
        return invalid(
            &identity.path,
            "centralized components require AHCL 1.2 and the parent materials directory".to_owned(),
        );
    }
    Ok(())
}

fn narrowed_license(
    parent: &LicenseSettings,
    identity: &ComponentIdentity,
    project: &ProjectSettings,
    local: Option<&HistoryOverrides>,
) -> Result<LicenseSettings, ConfigError> {
    let version = identity.license_version.unwrap_or(parent.version);
    let covered_scope = identity
        .covered_scope
        .clone()
        .unwrap_or_else(|| identity.package.clone());
    let history = component_history(parent, version, &covered_scope, project.adoption_date);
    let mut license = LicenseSettings {
        version,
        enabled: identity.enabled,
        covered_scope,
        special_authorization_channel: identity
            .special_authorization_channel
            .clone()
            .unwrap_or_else(|| parent.special_authorization_channel.clone()),
        history,
    };
    license.history = license
        .history
        .with_overrides(local, &license, project, &identity.path)?;
    Ok(license)
}

fn component_history(
    parent: &LicenseSettings,
    version: AhclVersion,
    scope: &str,
    adoption: Option<UtcDate>,
) -> LicensingHistory {
    if matching_scope(scope) != matching_scope(parent.covered_scope()) {
        return LicensingHistory::initial(adoption);
    }
    if version == parent.version {
        parent.history.clone()
    } else {
        parent.history.without_scalar_dates()
    }
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
