// crates/ahcl-kit-materials/src/third_party.rs - Managed third party material planning and state.
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

use crate::dependencies::{evidence_basenames, package_directories, sha256_hex, validate_basename};
use crate::{LayoutPolicy, MaterialsError, MaterialsErrorCode};
use ahcl_kit_config::EffectiveConfig;
use ahcl_kit_core::{
    ChangePlan, Diagnostic, DiagnosticCode, DiagnosticSeverity, ProjectEntry, ProjectView,
    RepoPath, ResolvedGraph,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const MANAGED_STATE_BASENAME: &str = ".ahcl-kit-state.json";
pub(crate) const MANAGED_STAGING_BASENAME: &str = ".ahcl-kit-staging";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedEntryKind {
    Absent,
    File,
    Directory,
    LinkOrReparsePoint,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedEvidenceInventory {
    basename: String,
    kind: ManagedEntryKind,
}

impl ManagedEvidenceInventory {
    pub fn new(basename: impl Into<String>, kind: ManagedEntryKind) -> Self {
        Self {
            basename: basename.into(),
            kind,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedPackageInventory {
    directory: String,
    kind: ManagedEntryKind,
    evidence: Vec<ManagedEvidenceInventory>,
}

impl ManagedPackageInventory {
    pub fn new(
        directory: impl Into<String>,
        kind: ManagedEntryKind,
        evidence: Vec<ManagedEvidenceInventory>,
    ) -> Self {
        Self {
            directory: directory.into(),
            kind,
            evidence,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedRootInventoryEntry {
    name: String,
    kind: ManagedEntryKind,
}

impl ManagedRootInventoryEntry {
    pub fn new(name: impl Into<String>, kind: ManagedEntryKind) -> Self {
        Self {
            name: name.into(),
            kind,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedThirdPartyInventory {
    root_kind: ManagedEntryKind,
    state_kind: ManagedEntryKind,
    staging_kind: ManagedEntryKind,
    packages: Vec<ManagedPackageInventory>,
    extra_root_entries: Vec<ManagedRootInventoryEntry>,
}

impl ManagedThirdPartyInventory {
    pub fn absent() -> Self {
        Self::new(
            ManagedEntryKind::Absent,
            ManagedEntryKind::Absent,
            ManagedEntryKind::Absent,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn new(
        root_kind: ManagedEntryKind,
        state_kind: ManagedEntryKind,
        staging_kind: ManagedEntryKind,
        packages: Vec<ManagedPackageInventory>,
        extra_root_entries: Vec<ManagedRootInventoryEntry>,
    ) -> Self {
        Self {
            root_kind,
            state_kind,
            staging_kind,
            packages,
            extra_root_entries,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedRemoval {
    Evidence {
        package_directory: String,
        evidence_basename: String,
    },
    PackageDirectory {
        package_directory: String,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MaterialGenerationPlan {
    changes: ChangePlan,
    managed_removals: Vec<ManagedRemoval>,
    diagnostics: Vec<Diagnostic>,
}

impl MaterialGenerationPlan {
    pub fn changes(&self) -> &ChangePlan {
        &self.changes
    }

    pub fn managed_removals(&self) -> &[ManagedRemoval] {
        &self.managed_removals
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub(crate) fn new(
        changes: ChangePlan,
        managed_removals: Vec<ManagedRemoval>,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        Self {
            changes,
            managed_removals,
            diagnostics,
        }
    }

    pub(crate) fn into_parts(self) -> (ChangePlan, Vec<ManagedRemoval>, Vec<Diagnostic>) {
        (self.changes, self.managed_removals, self.diagnostics)
    }
}

pub struct ThirdPartyMaterialGenerator;

impl ThirdPartyMaterialGenerator {
    pub fn plan_tree(
        view: &dyn ProjectView,
        config: &EffectiveConfig,
        graph: &ResolvedGraph,
        inventory: &ManagedThirdPartyInventory,
    ) -> Result<MaterialGenerationPlan, MaterialsError> {
        validate_inventory(inventory)?;
        let layout = LayoutPolicy::from_config(config)?;
        let base = third_party_base(&layout)?;
        let previous = read_state(view, &base, inventory.state_kind)?;
        let desired = desired_state_and_writes(graph, &base)?;
        validate_replacement_targets(inventory, &desired.manifest)?;
        let (managed_removals, diagnostics) =
            cleanup_plan(view, &base, inventory, &previous, &desired.manifest)?;
        let changes = desired
            .writes
            .compare(view)
            .map_err(MaterialsError::from_plan)?;
        Ok(MaterialGenerationPlan::new(
            changes,
            managed_removals,
            diagnostics,
        ))
    }
}

struct DesiredTree {
    manifest: StateManifest,
    writes: ChangePlan,
}

fn desired_state_and_writes(
    graph: &ResolvedGraph,
    base: &RepoPath,
) -> Result<DesiredTree, MaterialsError> {
    let directories = package_directories(graph);
    let mut packages = graph
        .packages
        .iter()
        .filter(|package| !package.first_party)
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| left.id.cmp(&right.id));
    let mut state_packages = Vec::new();
    let mut writes = ChangePlan::new();
    for package in packages {
        let directory = directories
            .get(&package.id)
            .ok_or_else(|| MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid))?;
        validate_managed_package_identity(directory)?;
        let basenames = evidence_basenames(package)?;
        let mut evidence = Vec::new();
        for (artifact, basename) in package.license_artifacts.iter().zip(basenames) {
            writes
                .write(
                    managed_path(base, directory, Some(&basename))?,
                    artifact.bytes.clone(),
                )
                .map_err(MaterialsError::from_plan)?;
            evidence.push(StateEvidence {
                basename,
                sha256: sha256_hex(&artifact.bytes),
            });
        }
        evidence.sort_by(|left, right| left.basename.cmp(&right.basename));
        state_packages.push(StatePackage {
            directory: directory.clone(),
            evidence,
        });
    }
    state_packages.sort_by(|left, right| left.directory.cmp(&right.directory));
    let manifest = StateManifest {
        schema: 1,
        packages: state_packages,
    };
    let mut state_bytes = serde_json::to_string_pretty(&manifest)
        .map_err(|_| MaterialsError::new(MaterialsErrorCode::StateInvalid))?
        .into_bytes();
    state_bytes.push(b'\n');
    writes
        .write(state_path(base)?, state_bytes)
        .map_err(MaterialsError::from_plan)?;
    Ok(DesiredTree { manifest, writes })
}

fn read_state(
    view: &dyn ProjectView,
    base: &RepoPath,
    state_kind: ManagedEntryKind,
) -> Result<StateManifest, MaterialsError> {
    match state_kind {
        ManagedEntryKind::Absent => Ok(StateManifest::empty()),
        ManagedEntryKind::File => {
            let entry = view
                .entry(&state_path(base)?)
                .map_err(|_| MaterialsError::new(MaterialsErrorCode::View))?;
            let ProjectEntry::File(bytes) = entry else {
                return Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid));
            };
            parse_state(&bytes)
        }
        ManagedEntryKind::LinkOrReparsePoint => {
            Err(MaterialsError::new(MaterialsErrorCode::LinkOrReparsePoint))
        }
        ManagedEntryKind::Directory | ManagedEntryKind::Other => {
            Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid))
        }
    }
}

fn parse_state(bytes: &[u8]) -> Result<StateManifest, MaterialsError> {
    let manifest: StateManifest = serde_json::from_slice(bytes)
        .map_err(|_| MaterialsError::new(MaterialsErrorCode::StateInvalid))?;
    if manifest.schema != 1 {
        return Err(MaterialsError::new(MaterialsErrorCode::StateInvalid));
    }
    let mut directories = BTreeSet::new();
    for package in &manifest.packages {
        validate_managed_package_identity(&package.directory)
            .map_err(|_| MaterialsError::new(MaterialsErrorCode::StateInvalid))?;
        if !directories.insert(package.directory.to_ascii_lowercase()) {
            return Err(MaterialsError::new(MaterialsErrorCode::StateInvalid));
        }
        let mut basenames = BTreeSet::new();
        for evidence in &package.evidence {
            validate_basename(&evidence.basename)
                .map_err(|_| MaterialsError::new(MaterialsErrorCode::StateInvalid))?;
            if !basenames.insert(evidence.basename.to_ascii_lowercase())
                || !valid_sha256(&evidence.sha256)
            {
                return Err(MaterialsError::new(MaterialsErrorCode::StateInvalid));
            }
        }
    }
    Ok(manifest)
}

fn validate_inventory(inventory: &ManagedThirdPartyInventory) -> Result<(), MaterialsError> {
    validate_root_kind(inventory.root_kind)?;
    validate_state_kind(inventory.state_kind)?;
    match inventory.staging_kind {
        ManagedEntryKind::Absent => {}
        ManagedEntryKind::LinkOrReparsePoint => {
            return Err(MaterialsError::new(MaterialsErrorCode::LinkOrReparsePoint));
        }
        ManagedEntryKind::File | ManagedEntryKind::Directory | ManagedEntryKind::Other => {
            return Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid));
        }
    }
    if inventory.root_kind == ManagedEntryKind::Absent
        && (inventory.state_kind != ManagedEntryKind::Absent
            || !inventory.packages.is_empty()
            || !inventory.extra_root_entries.is_empty())
    {
        return Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid));
    }
    let mut packages = BTreeSet::new();
    for package in &inventory.packages {
        validate_managed_package_identity(&package.directory)?;
        if !packages.insert(package.directory.to_ascii_lowercase()) {
            return Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid));
        }
        match package.kind {
            ManagedEntryKind::Directory | ManagedEntryKind::Absent => {}
            ManagedEntryKind::LinkOrReparsePoint => {
                return Err(MaterialsError::new(MaterialsErrorCode::LinkOrReparsePoint));
            }
            ManagedEntryKind::File | ManagedEntryKind::Other => {
                return Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid));
            }
        }
        let mut evidence = BTreeSet::new();
        for entry in &package.evidence {
            validate_basename(&entry.basename)?;
            if !evidence.insert(entry.basename.to_ascii_lowercase()) {
                return Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid));
            }
            if entry.kind == ManagedEntryKind::LinkOrReparsePoint {
                return Err(MaterialsError::new(MaterialsErrorCode::LinkOrReparsePoint));
            }
        }
    }
    let mut root_entries = BTreeSet::new();
    for entry in &inventory.extra_root_entries {
        validate_basename(&entry.name)?;
        let key = entry.name.to_ascii_lowercase();
        if matches!(
            key.as_str(),
            MANAGED_STATE_BASENAME | MANAGED_STAGING_BASENAME
        ) || !root_entries.insert(key)
        {
            return Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid));
        }
        if entry.kind == ManagedEntryKind::LinkOrReparsePoint {
            return Err(MaterialsError::new(MaterialsErrorCode::LinkOrReparsePoint));
        }
    }
    Ok(())
}

fn validate_replacement_targets(
    inventory: &ManagedThirdPartyInventory,
    desired: &StateManifest,
) -> Result<(), MaterialsError> {
    for package in &desired.packages {
        if let Some(observed) = inventory_package(inventory, &package.directory) {
            if observed.directory != package.directory {
                return Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid));
            }
            for evidence in &package.evidence {
                if let Some(entry) = inventory_evidence(observed, &evidence.basename) {
                    match entry.kind {
                        ManagedEntryKind::Absent | ManagedEntryKind::File => {}
                        ManagedEntryKind::LinkOrReparsePoint => {
                            return Err(MaterialsError::new(
                                MaterialsErrorCode::LinkOrReparsePoint,
                            ));
                        }
                        ManagedEntryKind::Directory | ManagedEntryKind::Other => {
                            return Err(MaterialsError::new(
                                MaterialsErrorCode::ManagedTreeInvalid,
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn cleanup_plan(
    view: &dyn ProjectView,
    base: &RepoPath,
    inventory: &ManagedThirdPartyInventory,
    previous: &StateManifest,
    desired: &StateManifest,
) -> Result<(Vec<ManagedRemoval>, Vec<Diagnostic>), MaterialsError> {
    let desired_entries = state_entries(desired);
    let previous_entries = state_entries(previous);
    let mut removals = Vec::new();
    let mut removed_entries = BTreeSet::new();
    let mut diagnostics = BTreeMap::new();

    for package in &previous.packages {
        let Some(observed) = inventory_package(inventory, &package.directory) else {
            continue;
        };
        for evidence in &package.evidence {
            let key = entry_key(&package.directory, &evidence.basename);
            if desired_entries.contains(&key) {
                continue;
            }
            let Some(entry) = inventory_evidence(observed, &evidence.basename) else {
                continue;
            };
            match entry.kind {
                ManagedEntryKind::Absent => {}
                ManagedEntryKind::File => {
                    let path = managed_path(base, &package.directory, Some(&evidence.basename))?;
                    let observed_entry = view
                        .entry(&path)
                        .map_err(|_| MaterialsError::new(MaterialsErrorCode::View))?;
                    let ProjectEntry::File(bytes) = observed_entry else {
                        return Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid));
                    };
                    if sha256_hex(&bytes) == evidence.sha256 {
                        removals.push(ManagedRemoval::Evidence {
                            package_directory: package.directory.clone(),
                            evidence_basename: evidence.basename.clone(),
                        });
                        removed_entries.insert(key);
                    } else {
                        add_unowned_diagnostic(&mut diagnostics, path);
                    }
                }
                ManagedEntryKind::LinkOrReparsePoint => {
                    return Err(MaterialsError::new(MaterialsErrorCode::LinkOrReparsePoint));
                }
                ManagedEntryKind::Directory | ManagedEntryKind::Other => {
                    return Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid));
                }
            }
        }
    }

    for package in &inventory.packages {
        for evidence in &package.evidence {
            if evidence.kind == ManagedEntryKind::Absent {
                continue;
            }
            let key = entry_key(&package.directory, &evidence.basename);
            if desired_entries.contains(&key) || removed_entries.contains(&key) {
                continue;
            }
            let path = managed_path(base, &package.directory, Some(&evidence.basename))?;
            if !previous_entries.contains(&key) || !diagnostics.contains_key(path.as_str()) {
                add_unowned_diagnostic(&mut diagnostics, path);
            }
        }
    }

    for entry in &inventory.extra_root_entries {
        if entry.kind != ManagedEntryKind::Absent {
            add_unowned_diagnostic(&mut diagnostics, root_entry_path(base, &entry.name)?);
        }
    }

    for package in &previous.packages {
        let Some(observed) = inventory_package(inventory, &package.directory) else {
            continue;
        };
        let has_desired = desired
            .packages
            .iter()
            .any(|candidate| candidate.directory.eq_ignore_ascii_case(&package.directory));
        let all_existing_removed = observed
            .evidence
            .iter()
            .filter(|entry| entry.kind != ManagedEntryKind::Absent)
            .all(|entry| removed_entries.contains(&entry_key(&package.directory, &entry.basename)));
        if !has_desired && all_existing_removed {
            removals.push(ManagedRemoval::PackageDirectory {
                package_directory: package.directory.clone(),
            });
        }
    }

    Ok((removals, diagnostics.into_values().collect()))
}

fn add_unowned_diagnostic(diagnostics: &mut BTreeMap<String, Diagnostic>, path: RepoPath) {
    diagnostics
        .entry(path.as_str().to_owned())
        .or_insert_with(|| {
            Diagnostic::new(
                DiagnosticCode::new("materials.third_party_unowned"),
                DiagnosticSeverity::Warning,
                "unowned third-party entry was preserved",
            )
            .at_path(path)
        });
}

fn state_entries(manifest: &StateManifest) -> BTreeSet<(String, String)> {
    manifest
        .packages
        .iter()
        .flat_map(|package| {
            package
                .evidence
                .iter()
                .map(|evidence| entry_key(&package.directory, &evidence.basename))
        })
        .collect()
}

fn entry_key(directory: &str, basename: &str) -> (String, String) {
    (
        directory.to_ascii_lowercase(),
        basename.to_ascii_lowercase(),
    )
}

fn inventory_package<'a>(
    inventory: &'a ManagedThirdPartyInventory,
    directory: &str,
) -> Option<&'a ManagedPackageInventory> {
    inventory
        .packages
        .iter()
        .find(|package| package.directory.eq_ignore_ascii_case(directory))
}

fn inventory_evidence<'a>(
    package: &'a ManagedPackageInventory,
    basename: &str,
) -> Option<&'a ManagedEvidenceInventory> {
    package
        .evidence
        .iter()
        .find(|entry| entry.basename.eq_ignore_ascii_case(basename))
}

fn validate_root_kind(kind: ManagedEntryKind) -> Result<(), MaterialsError> {
    match kind {
        ManagedEntryKind::Absent | ManagedEntryKind::Directory => Ok(()),
        ManagedEntryKind::LinkOrReparsePoint => {
            Err(MaterialsError::new(MaterialsErrorCode::LinkOrReparsePoint))
        }
        ManagedEntryKind::File | ManagedEntryKind::Other => {
            Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid))
        }
    }
}

fn validate_state_kind(kind: ManagedEntryKind) -> Result<(), MaterialsError> {
    match kind {
        ManagedEntryKind::Absent | ManagedEntryKind::File => Ok(()),
        ManagedEntryKind::LinkOrReparsePoint => {
            Err(MaterialsError::new(MaterialsErrorCode::LinkOrReparsePoint))
        }
        ManagedEntryKind::Directory | ManagedEntryKind::Other => {
            Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid))
        }
    }
}

pub(crate) fn validate_managed_package_identity(value: &str) -> Result<(), MaterialsError> {
    validate_basename(value)?;
    let portable = value.to_ascii_lowercase();
    if !value.is_ascii()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        || !value.as_bytes().contains(&b'-')
        || matches!(value, "LICENSE" | "COPYING" | "NOTICE" | "COPYRIGHT")
        || matches!(
            portable.as_str(),
            MANAGED_STATE_BASENAME | MANAGED_STAGING_BASENAME
        )
    {
        return Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid));
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn third_party_base(layout: &LayoutPolicy) -> Result<RepoPath, MaterialsError> {
    RepoPath::parse(format!(
        "{}/THIRD-PARTY-LICENSES",
        layout.materials_directory().as_str()
    ))
    .map_err(|_| MaterialsError::new(MaterialsErrorCode::InvalidLayout))
}

fn state_path(base: &RepoPath) -> Result<RepoPath, MaterialsError> {
    root_entry_path(base, MANAGED_STATE_BASENAME)
}

fn root_entry_path(base: &RepoPath, name: &str) -> Result<RepoPath, MaterialsError> {
    validate_basename(name)?;
    RepoPath::parse(format!("{}/{name}", base.as_str()))
        .map_err(|_| MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid))
}

fn managed_path(
    base: &RepoPath,
    directory: &str,
    basename: Option<&str>,
) -> Result<RepoPath, MaterialsError> {
    validate_managed_package_identity(directory)?;
    let value = match basename {
        Some(basename) => {
            validate_basename(basename)?;
            format!("{}/{directory}/{basename}", base.as_str())
        }
        None => format!("{}/{directory}", base.as_str()),
    };
    RepoPath::parse(value).map_err(|_| MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StateManifest {
    schema: u32,
    packages: Vec<StatePackage>,
}

impl StateManifest {
    fn empty() -> Self {
        Self {
            schema: 1,
            packages: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StatePackage {
    directory: String,
    evidence: Vec<StateEvidence>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StateEvidence {
    basename: String,
    sha256: String,
}

#[cfg(test)]
mod tests {
    use super::validate_managed_package_identity;

    #[test]
    fn package_identity_rejects_reserved_names_case_insensitively() {
        for name in [
            ".ahcl-kit-state.json",
            ".AHCL-KIT-STATE.JSON",
            ".ahcl-kit-staging",
            ".AHCL-KIT-STAGING",
        ] {
            assert!(
                validate_managed_package_identity(name).is_err(),
                "accepted {name:?}"
            );
        }
        assert!(validate_managed_package_identity("package-1.0.0").is_ok());
    }
}
