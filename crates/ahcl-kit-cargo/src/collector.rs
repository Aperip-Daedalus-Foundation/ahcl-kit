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

use crate::{CargoError, EvidenceLimits};
use ahcl_kit_core::{LicenseArtifact, RepoPath};
use ahcl_kit_fs::{PackageDirectory, PackageFsError};
use cargo_metadata::Package;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

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
    let package_root = package_root(package, &package_id)?;
    // Package bytes come from the shared no-follow directory, not a path walk.
    let directory = PackageDirectory::open(package_root)
        .map_err(|error| map_package_fs_error(error, &package_id))?;
    let candidates = evidence_candidates(package, package_root, &directory, limits, &package_id)?;
    read_artifacts(candidates, &directory, limits, budget, &package_id)
}

fn package_root<'a>(package: &'a Package, package_id: &str) -> Result<&'a Path, CargoError> {
    Path::new(package.manifest_path.as_std_path())
        .parent()
        .ok_or_else(|| CargoError::EvidenceOutsidePackage {
            package_id: package_id.to_owned(),
        })
}

fn evidence_candidates(
    package: &Package,
    package_root: &Path,
    directory: &PackageDirectory,
    limits: EvidenceLimits,
    package_id: &str,
) -> Result<Vec<PathBuf>, CargoError> {
    let mut candidates = Vec::new();
    push_declared_license(&mut candidates, package, package_root, package_id)?;
    candidates.extend(
        directory
            .root_license_candidates(limits.max_files_per_package())
            .map_err(|error| map_package_fs_error(error, package_id))?,
    );
    dedup_paths(&mut candidates);
    if candidate_count_exceeds(&candidates, limits) {
        return Err(CargoError::TooManyEvidenceFiles {
            package_id: package_id.to_owned(),
            count: u64::try_from(candidates.len()).unwrap_or(u64::MAX),
        });
    }
    Ok(candidates)
}

fn push_declared_license(
    candidates: &mut Vec<PathBuf>,
    package: &Package,
    package_root: &Path,
    package_id: &str,
) -> Result<(), CargoError> {
    let Some(license_file) = &package.license_file else {
        return Ok(());
    };
    let path = license_file.as_std_path();
    let relative = if path.is_absolute() {
        path.strip_prefix(package_root)
            .map_err(|_| CargoError::EvidenceOutsidePackage {
                package_id: package_id.to_owned(),
            })?
            .to_path_buf()
    } else {
        path.to_path_buf()
    };
    candidates.push(relative);
    Ok(())
}

fn dedup_paths(candidates: &mut Vec<PathBuf>) {
    let mut seen = BTreeSet::new();
    candidates.retain(|path| {
        seen.insert(
            path.to_string_lossy()
                .replace('\\', "/")
                .to_ascii_lowercase(),
        )
    });
}

fn candidate_count_exceeds(candidates: &[PathBuf], limits: EvidenceLimits) -> bool {
    u64::try_from(candidates.len()).map_or(true, |count| count > limits.max_files_per_package())
}

fn read_artifacts(
    candidates: Vec<PathBuf>,
    directory: &PackageDirectory,
    limits: EvidenceLimits,
    budget: &mut EvidenceBudget,
    package_id: &str,
) -> Result<Vec<LicenseArtifact>, CargoError> {
    let mut artifacts = Vec::new();
    let mut file_count = 0_u64;
    let mut read = ArtifactRead {
        directory,
        limits,
        budget,
        package_id,
        artifacts: &mut artifacts,
    };
    for relative in candidates {
        file_count = push_artifact(&relative, file_count, &mut read)?;
    }
    Ok(artifacts)
}

struct ArtifactRead<'a> {
    directory: &'a PackageDirectory,
    limits: EvidenceLimits,
    budget: &'a mut EvidenceBudget,
    package_id: &'a str,
    artifacts: &'a mut Vec<LicenseArtifact>,
}

fn push_artifact(
    relative: &Path,
    file_count: u64,
    read: &mut ArtifactRead<'_>,
) -> Result<u64, CargoError> {
    validate_relative(relative, read.package_id)?;
    let Some(bytes) = read
        .directory
        .read_bounded_file(relative, read.limits.max_file_bytes())
        .map_err(|error| map_package_fs_error(error, read.package_id))?
    else {
        return Ok(file_count);
    };
    let file_count = file_count.saturating_add(1);
    accept_file_count(file_count, read.limits, read.package_id)?;
    charge_budget(read.budget, read.limits, bytes.len())?;
    read.artifacts.push(LicenseArtifact {
        relative_path: artifact_repo_path(relative, read.package_id)?,
        bytes,
    });
    Ok(file_count)
}

fn accept_file_count(
    file_count: u64,
    limits: EvidenceLimits,
    package_id: &str,
) -> Result<(), CargoError> {
    if file_count > limits.max_files_per_package() {
        return Err(CargoError::TooManyEvidenceFiles {
            package_id: package_id.to_owned(),
            count: file_count,
        });
    }
    Ok(())
}

fn charge_budget(
    budget: &mut EvidenceBudget,
    limits: EvidenceLimits,
    byte_len: usize,
) -> Result<(), CargoError> {
    let byte_len = u64::try_from(byte_len).unwrap_or(u64::MAX);
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
    Ok(())
}

fn artifact_repo_path(relative: &Path, package_id: &str) -> Result<RepoPath, CargoError> {
    let relative = relative
        .to_str()
        .ok_or_else(|| CargoError::EvidencePathEncoding {
            package_id: package_id.to_owned(),
        })?
        .replace('\\', "/");
    RepoPath::parse(&relative).map_err(|_| CargoError::InvalidRepositoryPath { path: relative })
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
