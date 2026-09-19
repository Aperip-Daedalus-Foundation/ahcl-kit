// crates/ahcl-kit-materials/src/apply.rs - Material plan application and managed removals.
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

use crate::dependencies::validate_basename;
use crate::third_party::validate_managed_package_identity;
use crate::view::{ManagedThirdPartyDir, ProjectFilesystem};
use crate::{ManagedRemoval, MaterialsError, MaterialsErrorCode, SafeRelPath};
use ahcl_kit_core::{ChangeKind, ChangePlan, RepoPath};

pub struct PlanApplier;

impl PlanApplier {
    pub fn apply(
        filesystem: &ProjectFilesystem,
        compared_plan: &ChangePlan,
    ) -> Result<(), MaterialsError> {
        let mut writes = Vec::new();
        for change in compared_plan.changes() {
            if change.kind() == ChangeKind::Remove {
                return Err(MaterialsError::root(
                    "materials.apply.remove_forbidden",
                    "general planned removals are forbidden",
                ));
            }
            let path = SafeRelPath::from_repo_path(change.path())?;
            let bytes = change.bytes().ok_or_else(|| {
                MaterialsError::at_path(
                    "materials.apply.missing_bytes",
                    "planned write has no content",
                    &path,
                )
            })?;
            writes.push((path, change.kind(), bytes));
        }

        for (path, kind, bytes) in writes {
            let replace = kind == ChangeKind::Replace;
            filesystem.root.atomic_write(&path, bytes, replace)?;
        }
        Ok(())
    }

    pub fn apply_managed_removals(
        managed: &ManagedThirdPartyDir<'_>,
        removals: &[ManagedRemoval],
    ) -> Result<(), MaterialsError> {
        if removals.is_empty() {
            return Ok(());
        }

        let mut validated = Vec::with_capacity(removals.len());
        for removal in removals {
            validated.push(ValidatedManagedRemoval::from_removal(removal)?);
        }
        managed.ensure_present()?;

        for removal in validated {
            match removal {
                ValidatedManagedRemoval::Evidence(path) => managed.remove_evidence(&path)?,
                ValidatedManagedRemoval::PackageDirectory(path) => {
                    managed.remove_empty_package(&path)?;
                }
            }
        }
        Ok(())
    }
}

enum ValidatedManagedRemoval {
    Evidence(SafeRelPath),
    PackageDirectory(SafeRelPath),
}

impl ValidatedManagedRemoval {
    fn from_removal(removal: &ManagedRemoval) -> Result<Self, MaterialsError> {
        match removal {
            ManagedRemoval::Evidence {
                package_directory,
                evidence_basename,
            } => {
                validate_package_directory(package_directory)?;
                validate_evidence_basename(evidence_basename)?;
                Ok(Self::Evidence(managed_path(&[
                    package_directory,
                    evidence_basename,
                ])?))
            }
            ManagedRemoval::PackageDirectory { package_directory } => {
                validate_package_directory(package_directory)?;
                Ok(Self::PackageDirectory(managed_path(&[package_directory])?))
            }
        }
    }
}

fn validate_package_directory(value: &str) -> Result<(), MaterialsError> {
    validate_managed_package_identity(value)
}

fn validate_evidence_basename(value: &str) -> Result<(), MaterialsError> {
    validate_basename(value)
}

fn managed_path(components: &[&str]) -> Result<SafeRelPath, MaterialsError> {
    let path = RepoPath::parse(components.join("/")).map_err(|_| managed_tree_error())?;
    SafeRelPath::from_repo_path(&path).map_err(|_| managed_tree_error())
}

fn managed_tree_error() -> MaterialsError {
    MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid)
}
