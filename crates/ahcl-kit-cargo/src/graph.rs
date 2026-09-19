// crates/ahcl-kit-cargo/src/graph.rs - Cargo metadata graph construction.
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

use crate::adapter::{CargoError, CargoResolveRequest};
use crate::collector::{self, EvidenceBudget};
use crate::platform_fs;
use ahcl_kit_config::CargoRuleClassification;
use ahcl_kit_core::{
    DependencyEdge, DependencyKind, LockfileEvidence, RepoPath, ResolvedGraph, ResolvedPackage,
};
use cargo_metadata::{
    DependencyKind as MetadataDependencyKind, Metadata, MetadataCommand, Package,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

pub(crate) fn resolve(request: &CargoResolveRequest) -> Result<ResolvedGraph, CargoError> {
    let mut packages = BTreeMap::<String, ResolvedPackage>::new();
    let mut edges = BTreeMap::<EdgeKey, bool>::new();
    let mut budget = EvidenceBudget::new();
    let mut seen_manifests = BTreeSet::new();
    let mut matched_selections = BTreeSet::new();

    for manifest in request.manifests() {
        if !seen_manifests.insert(manifest.as_str().to_ascii_lowercase()) {
            continue;
        }
        let metadata = metadata_for(request, manifest)?;
        merge_metadata(
            request,
            manifest,
            &metadata,
            &mut packages,
            &mut edges,
            &mut budget,
            &mut matched_selections,
        )?;
    }

    for selection in request.packages() {
        if !matched_selections.contains(selection) {
            return Err(CargoError::PackageSelection {
                package: selection.clone(),
            });
        }
    }

    let mut packages = packages.into_values().collect::<Vec<_>>();
    packages.sort_by(|left, right| left.id.cmp(&right.id));
    for package in &mut packages {
        package
            .contributing_lockfiles
            .sort_by(|left, right| left.path.cmp(&right.path));
    }
    let edges = edges
        .into_iter()
        .map(|(key, direct)| DependencyEdge {
            from_package_id: key.from,
            to_package_id: key.to,
            kind: key.kind.into_core(),
            target_conditions: key.target.into_iter().collect(),
            direct,
        })
        .collect();
    Ok(ResolvedGraph { packages, edges })
}

fn metadata_for(
    request: &CargoResolveRequest,
    manifest: &RepoPath,
) -> Result<Metadata, CargoError> {
    let manifest_path = request.project_root().resolve(manifest);
    let mut command = MetadataCommand::new();
    command
        .current_dir(request.project_root().as_path())
        .manifest_path(&manifest_path)
        .other_options(vec!["--locked".to_owned()]);
    command.exec().map_err(|error| CargoError::Metadata {
        manifest: manifest.clone(),
        message: error.to_string(),
    })
}

fn merge_metadata(
    request: &CargoResolveRequest,
    manifest: &RepoPath,
    metadata: &Metadata,
    packages: &mut BTreeMap<String, ResolvedPackage>,
    edges: &mut BTreeMap<EdgeKey, bool>,
    budget: &mut EvidenceBudget,
    matched_selections: &mut BTreeSet<String>,
) -> Result<(), CargoError> {
    let resolve = metadata
        .resolve
        .as_ref()
        .ok_or_else(|| CargoError::MissingResolve {
            manifest: manifest.clone(),
        })?;
    let package_by_id: HashMap<_, _> = metadata
        .packages
        .iter()
        .map(|package| (package.id.to_string(), package))
        .collect();
    let node_by_id: HashMap<_, _> = resolve
        .nodes
        .iter()
        .map(|node| (node.id.to_string(), node))
        .collect();
    let workspace_ids: BTreeSet<_> = metadata
        .workspace_members
        .iter()
        .map(ToString::to_string)
        .collect();
    let roots = selected_roots(request, metadata, &package_by_id, matched_selections)?;
    let root_ids: BTreeSet<_> = roots.iter().cloned().collect();
    let mut reachable = BTreeSet::new();
    let mut queue: VecDeque<_> = roots.into_iter().collect();

    while let Some(package_id) = queue.pop_front() {
        if !reachable.insert(package_id.clone()) {
            continue;
        }
        let node = node_by_id
            .get(&package_id)
            .ok_or_else(|| CargoError::MissingNode {
                package_id: package_id.clone(),
            })?;
        for dependency in &node.deps {
            let dependency_id = dependency.pkg.to_string();
            queue.push_back(dependency_id.clone());
            for kind in &dependency.dep_kinds {
                let Some(kind_key) = EdgeKind::from_metadata(kind.kind) else {
                    continue;
                };
                let key = EdgeKey {
                    from: package_id.clone(),
                    to: dependency_id.clone(),
                    kind: kind_key,
                    target: kind.target.as_ref().map(ToString::to_string),
                };
                let direct = edges.entry(key).or_default();
                *direct |= root_ids.contains(&package_id);
            }
        }
    }

    let lockfile = lockfile_evidence(request, metadata)?;
    let mut classifications = BTreeMap::new();
    for package_id in &reachable {
        let package = package_by_id
            .get(package_id)
            .ok_or_else(|| CargoError::MissingPackage {
                package_id: package_id.clone(),
            })?;
        let classification = if workspace_ids.contains(package_id) {
            CargoRuleClassification::FirstParty
        } else {
            let source = package
                .source
                .as_ref()
                .map_or_else(|| "path".to_owned(), ToString::to_string);
            request.classify(&format!("{}@{}", package.name, package.version), &source)
        };
        classifications.insert(package_id.clone(), classification);
        if classification == CargoRuleClassification::Exclude {
            continue;
        }

        if let Some(existing) = packages.get_mut(package_id) {
            if !existing
                .contributing_lockfiles
                .iter()
                .any(|current| current.path == lockfile.path)
            {
                existing.contributing_lockfiles.push(lockfile.clone());
            }
            continue;
        }
        let first_party = classification == CargoRuleClassification::FirstParty;
        let license_artifacts = if first_party {
            Vec::new()
        } else {
            let artifacts = collector::collect(package, request.limits(), budget)?;
            if artifacts.is_empty() && request.strict_license_files() {
                return Err(CargoError::MissingLicenseEvidence {
                    package_id: package_id.clone(),
                });
            }
            artifacts
        };
        packages.insert(
            package_id.clone(),
            resolved_package(package, first_party, lockfile.clone(), license_artifacts),
        );
    }

    edges.retain(|key, _| {
        classifications.get(&key.to) != Some(&CargoRuleClassification::Exclude)
            && classifications.get(&key.from) != Some(&CargoRuleClassification::Exclude)
    });
    Ok(())
}

fn selected_roots(
    request: &CargoResolveRequest,
    metadata: &Metadata,
    packages: &HashMap<String, &Package>,
    matched_selections: &mut BTreeSet<String>,
) -> Result<Vec<String>, CargoError> {
    if request.packages().is_empty() {
        if metadata.workspace_default_members.is_available()
            && !metadata.workspace_default_members.is_empty()
        {
            return Ok(metadata
                .workspace_default_members
                .iter()
                .map(ToString::to_string)
                .collect());
        }
        return Ok(metadata
            .workspace_members
            .iter()
            .map(ToString::to_string)
            .collect());
    }

    let workspace: BTreeSet<_> = metadata
        .workspace_members
        .iter()
        .map(ToString::to_string)
        .collect();
    let mut selected = Vec::new();
    for selection in request.packages() {
        let matches = workspace
            .iter()
            .filter(|package_id| {
                package_id.as_str() == selection
                    || packages
                        .get(package_id.as_str())
                        .is_some_and(|package| package.name.as_str() == selection)
            })
            .cloned()
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            return Err(CargoError::PackageSelection {
                package: selection.clone(),
            });
        }
        if let Some(package_id) = matches.into_iter().next() {
            matched_selections.insert(selection.clone());
            selected.push(package_id);
        }
    }
    selected.sort();
    selected.dedup();
    Ok(selected)
}

fn lockfile_evidence(
    request: &CargoResolveRequest,
    metadata: &Metadata,
) -> Result<LockfileEvidence, CargoError> {
    let lockfile = metadata.workspace_root.as_std_path().join("Cargo.lock");
    let relative = lockfile
        .strip_prefix(request.project_root().as_path())
        .map_err(|_| CargoError::LockfileOutsideProject)?;
    let relative_text = relative
        .to_str()
        .ok_or_else(|| CargoError::InvalidRepositoryPath {
            path: "Cargo.lock".to_owned(),
        })?
        .replace('\\', "/");
    let path = RepoPath::parse(&relative_text).map_err(|_| CargoError::InvalidRepositoryPath {
        path: relative_text,
    })?;
    let bytes =
        platform_fs::read_regular_file(&lockfile).map_err(|error| CargoError::LockfileRead {
            path: lockfile,
            source: error.into_io_error(),
        })?;
    Ok(LockfileEvidence {
        path,
        sha256: sha256_hex(&bytes),
        byte_len: bytes.len() as u64,
    })
}

fn resolved_package(
    package: &Package,
    first_party: bool,
    lockfile: LockfileEvidence,
    license_artifacts: Vec<ahcl_kit_core::LicenseArtifact>,
) -> ResolvedPackage {
    ResolvedPackage {
        id: package.id.to_string(),
        name: package.name.to_string(),
        version: package.version.to_string(),
        source: package.source.as_ref().map(ToString::to_string),
        checksum: None,
        manifest_path: package.manifest_path.as_std_path().to_path_buf(),
        repository: package.repository.clone(),
        homepage: package.homepage.clone(),
        authors: package.authors.clone(),
        declared_license: package.license.clone(),
        first_party,
        contributing_lockfiles: vec![lockfile],
        license_artifacts,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut result = String::with_capacity(64);
    for byte in digest {
        result.push(hex_digit(byte >> 4));
        result.push(hex_digit(byte & 0x0f));
    }
    result
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0' + value),
        _ => char::from(b'a' + value - 10),
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct EdgeKey {
    from: String,
    to: String,
    kind: EdgeKind,
    target: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum EdgeKind {
    Normal,
    Build,
    Development,
}

impl EdgeKind {
    fn from_metadata(kind: MetadataDependencyKind) -> Option<Self> {
        match kind {
            MetadataDependencyKind::Normal => Some(Self::Normal),
            MetadataDependencyKind::Build => Some(Self::Build),
            MetadataDependencyKind::Development => Some(Self::Development),
            _ => None,
        }
    }

    const fn into_core(self) -> DependencyKind {
        match self {
            Self::Normal => DependencyKind::Normal,
            Self::Build => DependencyKind::Build,
            Self::Development => DependencyKind::Development,
        }
    }
}
