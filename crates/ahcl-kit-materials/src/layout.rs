// crates/ahcl-kit-materials/src/layout.rs - AHCL material layout policy.
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

use crate::{MaterialsError, MaterialsErrorCode};
use ahcl_kit_config::{AhclVersion, EffectiveConfig};
use ahcl_kit_core::RepoPath;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayoutPolicy {
    version: AhclVersion,
    materials_directory: RepoPath,
}

impl LayoutPolicy {
    pub fn from_config(config: &EffectiveConfig) -> Result<Self, MaterialsError> {
        Self::new(
            config.license().version(),
            config.materials_directory().clone(),
        )
    }

    pub fn new(
        version: AhclVersion,
        materials_directory: RepoPath,
    ) -> Result<Self, MaterialsError> {
        let valid = match version {
            AhclVersion::V1_0 => materials_directory.as_str() == "AHCL",
            AhclVersion::V1_1 => matches!(
                materials_directory.as_str(),
                "AHCL" | "licenses/AHCL" | ".AHCL" | ".ahcl"
            ),
        };
        if !valid {
            return Err(MaterialsError::new(MaterialsErrorCode::InvalidLayout));
        }
        Ok(Self {
            version,
            materials_directory,
        })
    }

    pub fn version(&self) -> AhclVersion {
        self.version
    }

    pub fn materials_directory(&self) -> &RepoPath {
        &self.materials_directory
    }

    pub fn official_license_path(&self, filename: &str) -> Result<RepoPath, MaterialsError> {
        if filename.contains(['/', '\\']) || filename.is_empty() {
            return Err(MaterialsError::new(MaterialsErrorCode::InvalidLayout));
        }
        self.material_path(filename)
    }

    pub fn project_notice_path(&self) -> Result<RepoPath, MaterialsError> {
        self.material_path("AHCL-PROJECT-NOTICE.md")
    }

    pub fn version_adoption_path(&self) -> Result<RepoPath, MaterialsError> {
        self.material_path("AHCL-VERSION-ADOPTION.md")
    }

    pub fn source_path(&self) -> Result<RepoPath, MaterialsError> {
        self.material_path("AHCL-SOURCE.md")
    }

    pub fn dependencies_path(&self) -> Result<RepoPath, MaterialsError> {
        self.material_path("AHCL-DEPENDENCIES.md")
    }

    pub fn restrictions_index_path(&self) -> Result<RepoPath, MaterialsError> {
        self.material_path("AHCL-RESTRICTIONS/INDEX.md")
    }

    pub fn special_authorizations_path(&self) -> Result<RepoPath, MaterialsError> {
        self.material_path("AHCL-SPECIAL-AUTHORIZATIONS.md")
    }

    pub(crate) fn requires_empty_restrictions_index(&self) -> bool {
        self.version == AhclVersion::V1_0
    }

    pub(crate) fn requires_special_authorizations_placeholder(&self) -> bool {
        self.version == AhclVersion::V1_0
    }

    fn material_path(&self, relative: &str) -> Result<RepoPath, MaterialsError> {
        RepoPath::parse(format!("{}/{relative}", self.materials_directory.as_str()))
            .map_err(|_| MaterialsError::new(MaterialsErrorCode::InvalidLayout))
    }
}
