// crates/ahcl-kit-materials/src/platform_fs.rs - Platform filesystem backend selection and limits.
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

// Materials policy over `ahcl-kit-fs`. Platform opens, renames, and deletes live
// in that crate. This module maps those outcomes onto material errors, checks
// evidence digests, and classifies the managed third-party tree.

mod inventory;

use crate::{ManagedThirdPartyInventory, MaterialsError, MaterialsErrorCode, SafeRelPath};
use ahcl_kit_core::ProjectEntry;
use ahcl_kit_fs::{
    FsDirectory, FsRoot, PathFailure, ReadNode, RemoveFailure, RemoveStatus, RootFailure,
    WriteFailure,
};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub(crate) const MAX_MANAGED_ROOT_ENTRIES: usize = 2_048;
pub(crate) const MAX_MANAGED_PACKAGES: usize = 1_024;
pub(crate) const MAX_MANAGED_EVIDENCE_PER_PACKAGE: usize = 64;
pub(crate) const MAX_MANAGED_TOTAL_ENTRIES: usize = 8_192;

pub(crate) struct PlatformRoot {
    inner: FsRoot,
}

pub(crate) struct ManagedDirectory {
    inner: Option<FsDirectory>,
}

impl PlatformRoot {
    pub(crate) fn open(path: &Path) -> Result<Self, MaterialsError> {
        FsRoot::open(path)
            .map(|inner| Self { inner })
            .map_err(map_root)
    }

    pub(crate) fn read_entry(&self, path: &SafeRelPath) -> Result<ProjectEntry, MaterialsError> {
        match self
            .inner
            .read_entry(path.repo_path().as_path(), path.components())
        {
            Ok(node) => Ok(project_entry(node)),
            Err(error) => Err(map_path(path, error)),
        }
    }

    pub(crate) fn atomic_write(
        &self,
        path: &SafeRelPath,
        bytes: &[u8],
        replace: bool,
    ) -> Result<(), MaterialsError> {
        self.inner
            .write_file(path.components(), bytes, replace)
            .map_err(|error| map_write(path, error))
    }

    pub(crate) fn open_managed(
        &self,
        namespace: &SafeRelPath,
    ) -> Result<ManagedDirectory, MaterialsError> {
        match self.inner.open_directory(namespace.components()) {
            Ok(inner) => Ok(ManagedDirectory { inner }),
            Err(error) => Err(map_path(namespace, error)),
        }
    }
}

impl ManagedDirectory {
    pub(crate) fn inventory(&self) -> Result<ManagedThirdPartyInventory, MaterialsError> {
        match &self.inner {
            None => Ok(ManagedThirdPartyInventory::absent()),
            Some(directory) => inventory::inventory_directory(directory),
        }
    }

    pub(crate) fn ensure_present(&self) -> Result<(), MaterialsError> {
        if self.inner.is_some() {
            Ok(())
        } else {
            Err(managed_tree_error())
        }
    }

    pub(crate) fn remove_file(
        &self,
        path: &SafeRelPath,
        expected_sha256: &str,
    ) -> Result<(), MaterialsError> {
        let directory = self.inner.as_ref().ok_or_else(managed_tree_error)?;
        match directory.remove_file(path.components(), |file| {
            reader_matches_sha256(file, expected_sha256)
        }) {
            Ok(RemoveStatus::Removed | RemoveStatus::Absent) => Ok(()),
            Ok(RemoveStatus::Rejected) => Err(managed_changed_error(path)),
            Err(error) => Err(map_remove(path, error)),
        }
    }

    pub(crate) fn remove_empty_directory(&self, path: &SafeRelPath) -> Result<(), MaterialsError> {
        let directory = self.inner.as_ref().ok_or_else(managed_tree_error)?;
        match directory.remove_empty_directory(path.components()) {
            Ok(RemoveStatus::Removed | RemoveStatus::Absent) => Ok(()),
            Ok(RemoveStatus::Rejected) => Err(managed_changed_error(path)),
            Err(error) => Err(map_remove(path, error)),
        }
    }
}

fn project_entry(node: ReadNode) -> ProjectEntry {
    match node {
        ReadNode::Absent => ProjectEntry::Absent,
        ReadNode::Other => ProjectEntry::Other,
        ReadNode::File(bytes) => ProjectEntry::File(bytes),
    }
}

fn map_root(error: RootFailure) -> MaterialsError {
    match error {
        RootFailure::NotAbsolute => MaterialsError::root(
            "materials.root.not_absolute",
            "project root must be absolute",
        ),
        RootFailure::Invalid => invalid_root_error(),
        RootFailure::FilesystemRoot => MaterialsError::root(
            "materials.root.filesystem_root",
            "filesystem root cannot be a project root",
        ),
        RootFailure::Open => {
            MaterialsError::root("materials.root.open", "project root cannot be opened")
        }
        RootFailure::Reparse => root_reparse_error(),
        RootFailure::NotDirectory => MaterialsError::root(
            "materials.root.not_directory",
            "project root must be a directory",
        ),
    }
}

fn root_reparse_error() -> MaterialsError {
    #[cfg(windows)]
    let message = "project root cannot contain a link or reparse point";
    #[cfg(not(windows))]
    let message = "project root cannot contain a symbolic link";
    MaterialsError::root("materials.root.reparse", message)
}

fn map_path(path: &SafeRelPath, error: PathFailure) -> MaterialsError {
    match error {
        PathFailure::Reparse => reparse_error(path),
        PathFailure::NotDirectory => MaterialsError::at_path(
            "materials.path.not_directory",
            "path component is not a directory",
            path,
        ),
        PathFailure::Io => path_io_error(path),
        PathFailure::Internal => internal_error(),
    }
}

fn map_write(path: &SafeRelPath, error: WriteFailure) -> MaterialsError {
    match error {
        WriteFailure::Path(inner) => map_path(path, inner),
        WriteFailure::Commit => commit_error(path),
        WriteFailure::Write => MaterialsError::at_path(
            "materials.apply.write",
            "temporary file could not be written",
            path,
        ),
        WriteFailure::TempCreate => MaterialsError::at_path(
            "materials.apply.temp_create",
            "temporary file could not be created",
            path,
        ),
    }
}

fn map_remove(path: &SafeRelPath, error: RemoveFailure) -> MaterialsError {
    match error {
        RemoveFailure::Path(inner) => map_path(path, inner),
        RemoveFailure::ManagedLink => managed_link_error(),
        RemoveFailure::ManagedTree => managed_tree_error(),
        RemoveFailure::ManagedRemove => managed_remove_error(path),
    }
}

pub(crate) fn inventory_limit_error() -> MaterialsError {
    MaterialsError::filesystem(
        "materials.managed.inventory_limit",
        "managed third-party inventory exceeds supported entry limits",
    )
}

pub(crate) fn inventory_error() -> MaterialsError {
    MaterialsError::filesystem(
        "materials.managed.inventory",
        "managed third-party inventory could not be read",
    )
}

fn reader_matches_sha256(reader: &mut File, expected_sha256: &str) -> io::Result<bool> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let digest = digest.finalize();
    Ok(digest_matches(expected_sha256.as_bytes(), &digest, HEX))
}

fn digest_matches(expected: &[u8], digest: &[u8], hex: &[u8; 16]) -> bool {
    expected
        .chunks_exact(2)
        .zip(digest)
        .all(|(pair, actual)| hex_pair_matches(pair, *actual, hex))
}

fn hex_pair_matches(pair: &[u8], actual: u8, hex: &[u8; 16]) -> bool {
    pair[0] == hex[usize::from(actual >> 4)] && pair[1] == hex[usize::from(actual & 0x0f)]
}

fn invalid_root_error() -> MaterialsError {
    MaterialsError::root(
        "materials.root.invalid",
        "project root has an unsupported absolute-path form",
    )
}

fn internal_error() -> MaterialsError {
    MaterialsError::root(
        "materials.internal.invariant",
        "filesystem capability invariant failed",
    )
}

fn reparse_error(path: &SafeRelPath) -> MaterialsError {
    #[cfg(windows)]
    let message = "links and reparse points are forbidden";
    #[cfg(not(windows))]
    let message = "symbolic links are forbidden";
    MaterialsError::at_path("materials.path.reparse", message, path)
}

fn path_io_error(path: &SafeRelPath) -> MaterialsError {
    MaterialsError::at_path(
        "materials.path.io",
        "project entry could not be accessed",
        path,
    )
}

fn commit_error(path: &SafeRelPath) -> MaterialsError {
    MaterialsError::at_path("materials.apply.commit", "atomic file commit failed", path)
}

fn managed_tree_error() -> MaterialsError {
    MaterialsError::new(MaterialsErrorCode::ManagedTreeInvalid)
}

fn managed_link_error() -> MaterialsError {
    MaterialsError::new(MaterialsErrorCode::LinkOrReparsePoint)
}

fn managed_remove_error(path: &SafeRelPath) -> MaterialsError {
    MaterialsError::at_path(
        "materials.managed.remove",
        "managed entry could not be removed",
        path,
    )
}

fn managed_changed_error(path: &SafeRelPath) -> MaterialsError {
    MaterialsError::at_path(
        "materials.managed.changed",
        "managed evidence changed after planning",
        path,
    )
}
