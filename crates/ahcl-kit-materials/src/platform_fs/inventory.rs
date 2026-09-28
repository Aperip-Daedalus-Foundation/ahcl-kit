// crates/ahcl-kit-materials/src/platform_fs/inventory.rs - Managed third-party inventory policy.
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

use super::{
    MAX_MANAGED_EVIDENCE_PER_PACKAGE, MAX_MANAGED_PACKAGES, MAX_MANAGED_ROOT_ENTRIES,
    MAX_MANAGED_TOTAL_ENTRIES, inventory_error, inventory_limit_error,
};
use crate::dependencies::validate_basename;
use crate::third_party::{
    MANAGED_STAGING_BASENAME, MANAGED_STATE_BASENAME, validate_managed_package_identity,
};
use crate::{
    ManagedEntryKind, ManagedEvidenceInventory, ManagedPackageInventory, ManagedRootInventoryEntry,
    ManagedThirdPartyInventory, MaterialsError,
};
use ahcl_kit_fs::{FsDirectory, Inspected, ScanFailure};
use std::ffi::{OsStr, OsString};

pub(super) fn inventory_directory(
    root: &FsDirectory,
) -> Result<ManagedThirdPartyInventory, MaterialsError> {
    let root_entries = collect_names(root, MAX_MANAGED_ROOT_ENTRIES)?;
    let mut inventory = RootInventory::new(root_entries.len());
    for (name, os_name) in root_entries {
        // Package identity is checked before the entry is classified, and the
        // package cap is enforced before that entry is opened.
        inventory.record_root_entry(root, name, &os_name)?;
    }
    Ok(inventory.finish())
}

struct RootInventory {
    state_kind: ManagedEntryKind,
    staging_kind: ManagedEntryKind,
    packages: Vec<ManagedPackageInventory>,
    extra_root_entries: Vec<ManagedRootInventoryEntry>,
    remaining_total: usize,
}

impl RootInventory {
    fn new(root_entry_count: usize) -> Self {
        Self {
            state_kind: ManagedEntryKind::Absent,
            staging_kind: ManagedEntryKind::Absent,
            packages: Vec::new(),
            extra_root_entries: Vec::new(),
            remaining_total: MAX_MANAGED_TOTAL_ENTRIES - root_entry_count,
        }
    }

    fn record_root_entry(
        &mut self,
        root: &FsDirectory,
        name: String,
        os_name: &OsStr,
    ) -> Result<(), MaterialsError> {
        let is_package = validate_managed_package_identity(&name).is_ok();
        if package_limit_reached(is_package, self.packages.len()) {
            return Err(inventory_limit_error());
        }
        let (kind, directory) = inspect_entry(root, os_name)?;
        self.store_root_entry(name, is_package, kind, directory)
    }

    fn store_root_entry(
        &mut self,
        name: String,
        is_package: bool,
        kind: ManagedEntryKind,
        directory: Option<FsDirectory>,
    ) -> Result<(), MaterialsError> {
        match name.as_str() {
            MANAGED_STATE_BASENAME => self.state_kind = kind,
            MANAGED_STAGING_BASENAME => self.staging_kind = kind,
            _ if is_package => self.push_package(name, kind, directory)?,
            _ => self
                .extra_root_entries
                .push(ManagedRootInventoryEntry::new(name, kind)),
        }
        Ok(())
    }

    fn push_package(
        &mut self,
        name: String,
        kind: ManagedEntryKind,
        directory: Option<FsDirectory>,
    ) -> Result<(), MaterialsError> {
        let evidence = match directory {
            Some(directory) => self.read_package_evidence(&directory)?,
            None => Vec::new(),
        };
        self.packages
            .push(ManagedPackageInventory::new(name, kind, evidence));
        Ok(())
    }

    fn read_package_evidence(
        &mut self,
        directory: &FsDirectory,
    ) -> Result<Vec<ManagedEvidenceInventory>, MaterialsError> {
        let limit = MAX_MANAGED_EVIDENCE_PER_PACKAGE.min(self.remaining_total);
        let evidence_entries = collect_names(directory, limit)?;
        self.remaining_total -= evidence_entries.len();
        let mut evidence = Vec::new();
        evidence.reserve(evidence_entries.len());
        for (basename, os_basename) in evidence_entries {
            let (entry_kind, _) = inspect_entry(directory, &os_basename)?;
            evidence.push(ManagedEvidenceInventory::new(basename, entry_kind));
        }
        Ok(evidence)
    }

    fn finish(self) -> ManagedThirdPartyInventory {
        ManagedThirdPartyInventory::new(
            ManagedEntryKind::Directory,
            self.state_kind,
            self.staging_kind,
            self.packages,
            self.extra_root_entries,
        )
    }
}

fn package_limit_reached(is_package: bool, package_count: usize) -> bool {
    is_package && package_count == MAX_MANAGED_PACKAGES
}

fn collect_names(
    directory: &FsDirectory,
    max_entries: usize,
) -> Result<Vec<(String, OsString)>, MaterialsError> {
    let mut names = Vec::new();
    directory
        .scan_names(max_entries, |name| accept_name(name, &mut names))
        .map_err(map_scan)?;
    names.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(names)
}

fn accept_name(name: &OsStr, names: &mut Vec<(String, OsString)>) -> Result<(), MaterialsError> {
    let string = name.to_str().ok_or_else(inventory_error)?.to_owned();
    validate_basename(&string)?;
    names.push((string, name.to_os_string()));
    Ok(())
}

fn map_scan(error: ScanFailure<MaterialsError>) -> MaterialsError {
    match error {
        ScanFailure::Io => inventory_error(),
        ScanFailure::TooMany => inventory_limit_error(),
        ScanFailure::Visit(error) => error,
    }
}

fn inspect_entry(
    parent: &FsDirectory,
    name: &OsStr,
) -> Result<(ManagedEntryKind, Option<FsDirectory>), MaterialsError> {
    match parent.inspect(name).map_err(|_| inventory_error())? {
        Inspected::File => Ok((ManagedEntryKind::File, None)),
        Inspected::Link => Ok((ManagedEntryKind::LinkOrReparsePoint, None)),
        Inspected::Other => Ok((ManagedEntryKind::Other, None)),
        Inspected::Directory(directory) => Ok((ManagedEntryKind::Directory, Some(directory))),
    }
}
