// crates/ahcl-kit-cargo/src/collector.rs - Cargo license evidence collection.
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

use crate::platform_fs::{PackageDirectory, PackageFsError};
use crate::{CargoError, EvidenceLimits};
use ahcl_kit_core::{LicenseArtifact, RepoPath};
use cargo_metadata::Package;
use std::collections::BTreeSet;
use std::path::{Component, Path};

pub(crate) struct EvidenceBudget {
    aggregate_bytes: u64,
}

impl EvidenceBudget {
    pub(crate) const fn new() -> Self {
        Self { aggregate_bytes: 0 }
    }

    pub(crate) const fn aggregate_bytes(&self) -> u64 {
        self.aggregate_bytes
    }

    pub(crate) fn add(&mut self, bytes: u64) {
        self.aggregate_bytes = self.aggregate_bytes.saturating_add(bytes);
    }
}

pub(crate) fn collect(
    package: &Package,
    limits: EvidenceLimits,
    budget: &mut EvidenceBudget,
) -> Result<Vec<LicenseArtifact>, CargoError> {
    let package_id = package.id.to_string();
    let manifest_path = Path::new(package.manifest_path.as_std_path());
    let Some(package_root) = manifest_path.parent() else {
        return Err(CargoError::EvidenceOutsidePackage { package_id });
    };
    let directory = PackageDirectory::open(package_root)
        .map_err(|error| map_package_fs_error(error, &package_id))?;

    let mut candidates = Vec::new();
    if let Some(license_file) = &package.license_file {
        let path = license_file.as_std_path();
        let relative = if path.is_absolute() {
            path.strip_prefix(package_root)
                .map_err(|_| CargoError::EvidenceOutsidePackage {
                    package_id: package_id.clone(),
                })?
                .to_path_buf()
        } else {
            path.to_path_buf()
        };
        candidates.push(relative);
    }

    candidates.extend(
        directory
            .root_license_candidates(limits.max_files_per_package())
            .map_err(|error| map_package_fs_error(error, &package_id))?,
    );

    let mut seen = BTreeSet::new();
    candidates.retain(|path| {
        seen.insert(
            path.to_string_lossy()
                .replace('\\', "/")
                .to_ascii_lowercase(),
        )
    });
    if u64::try_from(candidates.len()).map_or(true, |count| count > limits.max_files_per_package())
    {
        return Err(CargoError::TooManyEvidenceFiles {
            package_id,
            count: u64::try_from(candidates.len()).unwrap_or(u64::MAX),
        });
    }
    let mut artifacts = Vec::new();
    let mut file_count = 0_u64;
    for relative in candidates {
        validate_relative(&relative, &package_id)?;
        let Some(bytes) = directory
            .read_bounded_file(&relative, limits.max_file_bytes())
            .map_err(|error| map_package_fs_error(error, &package_id))?
        else {
            continue;
        };
        file_count = file_count.saturating_add(1);
        if file_count > limits.max_files_per_package() {
            return Err(CargoError::TooManyEvidenceFiles {
                package_id: package_id.clone(),
                count: file_count,
            });
        }
        let byte_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let next_total = budget
            .aggregate_bytes
            .checked_add(byte_len)
            .ok_or(CargoError::AggregateEvidenceTooLarge { byte_len: u64::MAX })?;
        if next_total > limits.max_aggregate_bytes() {
            return Err(CargoError::AggregateEvidenceTooLarge {
                byte_len: next_total,
            });
        }
        budget.aggregate_bytes = next_total;
        let relative = relative
            .to_str()
            .ok_or_else(|| CargoError::EvidencePathEncoding {
                package_id: package_id.clone(),
            })?
            .replace('\\', "/");
        let relative_path =
            RepoPath::parse(&relative).map_err(|_| CargoError::InvalidRepositoryPath {
                path: relative.clone(),
            })?;
        artifacts.push(LicenseArtifact {
            relative_path,
            bytes,
        });
    }
    Ok(artifacts)
}

fn map_package_fs_error(error: PackageFsError, package_id: &str) -> CargoError {
    match error {
        PackageFsError::Io(source) => CargoError::EvidenceRead {
            package_id: package_id.to_owned(),
            source,
        },
        PackageFsError::InvalidPath => CargoError::EvidenceOutsidePackage {
            package_id: package_id.to_owned(),
        },
        PackageFsError::LinkOrReparsePoint => CargoError::LinkOrReparsePoint {
            package_id: package_id.to_owned(),
        },
        PackageFsError::PathEncoding => CargoError::EvidencePathEncoding {
            package_id: package_id.to_owned(),
        },
        PackageFsError::TooManyFiles(count) => CargoError::TooManyEvidenceFiles {
            package_id: package_id.to_owned(),
            count,
        },
        PackageFsError::FileTooLarge(byte_len) => CargoError::EvidenceFileTooLarge {
            package_id: package_id.to_owned(),
            byte_len,
        },
    }
}

fn validate_relative(path: &Path, package_id: &str) -> Result<(), CargoError> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(CargoError::EvidenceOutsidePackage {
            package_id: package_id.to_owned(),
        });
    }
    Ok(())
}
