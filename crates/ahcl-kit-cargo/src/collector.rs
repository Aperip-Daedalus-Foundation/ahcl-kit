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
use cargo_metadata::Package;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(crate) struct EvidenceBudget {
    aggregate_bytes: u64,
}

impl EvidenceBudget {
    pub(crate) const fn new() -> Self {
        Self { aggregate_bytes: 0 }
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
    reject_link(package_root, &package.id.to_string())?;

    let mut candidates = Vec::new();
    if let Some(license_file) = &package.license_file {
        let path = license_file.as_std_path();
        let relative = if path.is_absolute() {
            path.strip_prefix(package_root)
                .map_err(|_| CargoError::EvidenceOutsidePackage {
                    package_id: package.id.to_string(),
                })?
                .to_path_buf()
        } else {
            path.to_path_buf()
        };
        candidates.push(relative);
    }

    let mut root_candidates = Vec::new();
    let entries = fs::read_dir(package_root).map_err(|source| CargoError::EvidenceRead {
        package_id: package.id.to_string(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| CargoError::EvidenceRead {
            package_id: package.id.to_string(),
            source,
        })?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            return Err(CargoError::EvidencePathEncoding {
                package_id: package.id.to_string(),
            });
        };
        let upper = name.to_ascii_uppercase();
        if ["LICENSE", "COPYING", "NOTICE", "COPYRIGHT"]
            .iter()
            .any(|prefix| upper.starts_with(prefix))
        {
            root_candidates.push(PathBuf::from(name));
        }
    }
    root_candidates.sort_by(|left, right| {
        left.to_string_lossy()
            .to_ascii_lowercase()
            .cmp(&right.to_string_lossy().to_ascii_lowercase())
            .then_with(|| left.cmp(right))
    });
    candidates.extend(root_candidates);

    let mut seen = BTreeSet::new();
    candidates.retain(|path| {
        seen.insert(
            path.to_string_lossy()
                .replace('\\', "/")
                .to_ascii_lowercase(),
        )
    });
    let canonical_root =
        fs::canonicalize(package_root).map_err(|source| CargoError::EvidenceRead {
            package_id: package.id.to_string(),
            source,
        })?;
    let mut artifacts = Vec::new();
    let mut file_count = 0_u64;
    for relative in candidates {
        validate_relative(&relative, &package.id.to_string())?;
        let path = package_root.join(&relative);
        reject_path_links(package_root, &relative, &package.id.to_string())?;
        let canonical = fs::canonicalize(&path).map_err(|source| CargoError::EvidenceRead {
            package_id: package.id.to_string(),
            source,
        })?;
        if !canonical.starts_with(&canonical_root) {
            return Err(CargoError::EvidenceOutsidePackage {
                package_id: package.id.to_string(),
            });
        }
        let metadata = fs::metadata(&path).map_err(|source| CargoError::EvidenceRead {
            package_id: package.id.to_string(),
            source,
        })?;
        if !metadata.is_file() {
            continue;
        }
        file_count = file_count.saturating_add(1);
        if file_count > limits.max_files_per_package() {
            return Err(CargoError::TooManyEvidenceFiles {
                package_id: package.id.to_string(),
                count: file_count,
            });
        }
        if metadata.len() > limits.max_file_bytes() {
            return Err(CargoError::EvidenceFileTooLarge {
                package_id: package.id.to_string(),
                byte_len: metadata.len(),
            });
        }
        let next_total = budget
            .aggregate_bytes
            .checked_add(metadata.len())
            .ok_or(CargoError::AggregateEvidenceTooLarge { byte_len: u64::MAX })?;
        if next_total > limits.max_aggregate_bytes() {
            return Err(CargoError::AggregateEvidenceTooLarge {
                byte_len: next_total,
            });
        }
        let bytes = fs::read(&path).map_err(|source| CargoError::EvidenceRead {
            package_id: package.id.to_string(),
            source,
        })?;
        let byte_len = bytes.len() as u64;
        if byte_len != metadata.len() || byte_len > limits.max_file_bytes() {
            return Err(CargoError::EvidenceFileTooLarge {
                package_id: package.id.to_string(),
                byte_len,
            });
        }
        budget.aggregate_bytes = budget
            .aggregate_bytes
            .checked_add(byte_len)
            .ok_or(CargoError::AggregateEvidenceTooLarge { byte_len: u64::MAX })?;
        if budget.aggregate_bytes > limits.max_aggregate_bytes() {
            return Err(CargoError::AggregateEvidenceTooLarge {
                byte_len: budget.aggregate_bytes,
            });
        }
        let relative = relative
            .to_str()
            .ok_or_else(|| CargoError::EvidencePathEncoding {
                package_id: package.id.to_string(),
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

fn reject_path_links(root: &Path, relative: &Path, package_id: &str) -> Result<(), CargoError> {
    reject_link(root, package_id)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(CargoError::EvidenceOutsidePackage {
                package_id: package_id.to_owned(),
            });
        };
        current.push(part);
        reject_link(&current, package_id)?;
    }
    Ok(())
}

fn reject_link(path: &Path, package_id: &str) -> Result<(), CargoError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| CargoError::EvidenceRead {
        package_id: package_id.to_owned(),
        source,
    })?;
    if metadata.file_type().is_symlink() || is_reparse_point(&metadata) {
        return Err(CargoError::LinkOrReparsePoint {
            package_id: package_id.to_owned(),
        });
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}
