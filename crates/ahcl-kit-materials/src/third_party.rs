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
        expected_sha256: String,
    },
    PackageDirectory {
        package_directory: String,
    },
    /// Removes a legacy `.ahcl-kit-state.json` without interpreting its contents.
    LegacyState {
        expected_sha256: String,
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
        let (writes, diagnostics, managed_removals) =
            desired_writes(view, graph, &base, inventory)?;
        let changes = writes.compare(view).map_err(MaterialsError::from_plan)?;
        Ok(MaterialGenerationPlan::new(
            changes,
            managed_removals,
            diagnostics,
        ))
    }
}

fn desired_writes(
    view: &dyn ProjectView,
    graph: &ResolvedGraph,
    base: &RepoPath,
    inventory: &ManagedThirdPartyInventory,
) -> Result<(ChangePlan, Vec<Diagnostic>, Vec<ManagedRemoval>), MaterialsError> {
    let directories = package_directories(graph);
    let mut packages = graph
        .packages
        .iter()
        .filter(|package| !package.first_party)
        .collect::<Vec<_>>();
    packages.sort_by(|left, right| left.id.cmp(&right.id));
    let mut writes = ChangePlan::new();
    let mut diagnostics = BTreeMap::new();
    let mut desired_entries = BTreeSet::new();
    for package in packages {
        let directory = directories
            .get(&package.id)
            .ok_or_else(|| MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid))?;
        validate_managed_package_identity(directory)?;
        let basenames = evidence_basenames(package)?;
        for (artifact, basename) in package.license_artifacts.iter().zip(basenames) {
            desired_entries.insert(entry_key(directory, &basename));
            let path = managed_path(base, directory, Some(&basename))?;
            match view
                .entry(&path)
                .map_err(|_| MaterialsError::new(MaterialsErrorCode::View))?
            {
                ProjectEntry::Absent | ProjectEntry::File(_) => {
                    writes
                        .write(path, artifact.bytes.clone())
                        .map_err(MaterialsError::from_plan)?;
                }
                ProjectEntry::Other => {
                    return Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid));
                }
            }
        }
    }
    // Evidence outside the current graph stays in place. There is no ownership
    // ledger, so generate and check only converge the desired license files.
    for package in &inventory.packages {
        for evidence in &package.evidence {
            if evidence.kind == ManagedEntryKind::Absent {
                continue;
            }
            if desired_entries.contains(&entry_key(&package.directory, &evidence.basename)) {
                continue;
            }
            add_unowned_diagnostic(
                &mut diagnostics,
                managed_path(base, &package.directory, Some(&evidence.basename))?,
            );
        }
    }
    for entry in &inventory.extra_root_entries {
        if entry.kind != ManagedEntryKind::Absent {
            add_unowned_diagnostic(&mut diagnostics, root_entry_path(base, &entry.name)?);
        }
    }
    let managed_removals = legacy_state_removal(view, base, inventory.state_kind)?;
    Ok((
        writes,
        diagnostics.into_values().collect(),
        managed_removals,
    ))
}

fn legacy_state_removal(
    view: &dyn ProjectView,
    base: &RepoPath,
    state_kind: ManagedEntryKind,
) -> Result<Vec<ManagedRemoval>, MaterialsError> {
    if state_kind == ManagedEntryKind::Absent {
        return Ok(Vec::new());
    }
    let path = root_entry_path(base, MANAGED_STATE_BASENAME)?;
    match view
        .entry(&path)
        .map_err(|_| MaterialsError::new(MaterialsErrorCode::View))?
    {
        ProjectEntry::Absent => Ok(Vec::new()),
        ProjectEntry::File(bytes) => Ok(vec![ManagedRemoval::LegacyState {
            expected_sha256: sha256_hex(&bytes),
        }]),
        ProjectEntry::Other => Err(MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid)),
    }
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

fn entry_key(directory: &str, basename: &str) -> (String, String) {
    (
        directory.to_ascii_lowercase(),
        basename.to_ascii_lowercase(),
    )
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

pub(crate) fn valid_sha256(value: &str) -> bool {
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
