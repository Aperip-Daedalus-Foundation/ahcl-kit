// crates/ahcl-kit-materials/src/platform_fs/unix.rs - Unix capability scoped filesystem backend.
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

use super::{
    MAX_MANAGED_EVIDENCE_PER_PACKAGE, MAX_MANAGED_PACKAGES, MAX_MANAGED_ROOT_ENTRIES,
    MAX_MANAGED_TOTAL_ENTRIES, inventory_limit_error,
};
use crate::dependencies::validate_basename;
use crate::third_party::{
    MANAGED_STAGING_BASENAME, MANAGED_STATE_BASENAME, validate_managed_package_identity,
};
use crate::{
    ManagedEntryKind, ManagedEvidenceInventory, ManagedPackageInventory, ManagedRootInventoryEntry,
    ManagedThirdPartyInventory, MaterialsError, MaterialsErrorCode, SafeRelPath, is_dot_entry,
    platform_fs::reader_matches_sha256, temp_component,
};
use ahcl_kit_core::ProjectEntry;
use rustix::fd::OwnedFd;
use rustix::fs::{
    AtFlags, Dir, FileType, Mode, OFlags, Stat, fstat, fsync, linkat, mkdirat, open, openat,
    renameat, statat, unlinkat,
};
use rustix::io::{self as rio, Errno};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

const DIRECTORY_MODE: Mode = Mode::RWXU
    .union(Mode::RGRP)
    .union(Mode::XGRP)
    .union(Mode::ROTH)
    .union(Mode::XOTH);
const TEMP_MODE: Mode = Mode::RUSR.union(Mode::WUSR);
const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const FILE_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);
pub(crate) struct PlatformRoot {
    handle: OwnedFd,
}

pub(crate) struct ManagedDirectory {
    handle: Option<OwnedFd>,
}

enum DirectoryOpenError {
    Missing,
    Reparse,
    NotDirectory,
    Io,
}

enum NodeOpenError {
    Missing,
    Reparse,
    Io,
}

impl PlatformRoot {
    pub(crate) fn open(path: &Path) -> Result<Self, MaterialsError> {
        if !path.is_absolute() {
            return Err(MaterialsError::root(
                "materials.root.not_absolute",
                "project root must be absolute",
            ));
        }

        let mut components = Vec::new();
        let mut saw_root = false;
        for component in path.components() {
            match component {
                Component::RootDir if !saw_root => saw_root = true,
                Component::Normal(value) if saw_root => components.push(value.to_os_string()),
                _ => return Err(invalid_root_error()),
            }
        }
        if components.is_empty() {
            return Err(MaterialsError::root(
                "materials.root.filesystem_root",
                "filesystem root cannot be a project root",
            ));
        }

        let mut current = open(Path::new("/"), DIRECTORY_FLAGS, Mode::empty()).map_err(|_| {
            MaterialsError::root("materials.root.open", "project root cannot be opened")
        })?;
        for component in components {
            current = open_directory_at(&current, &component).map_err(map_root_directory_error)?;
        }
        Ok(Self { handle: current })
    }

    pub(crate) fn read_entry(&self, path: &SafeRelPath) -> Result<ProjectEntry, MaterialsError> {
        #[cfg(target_os = "linux")]
        if let Some(result) = read_entry_openat2(&self.handle, path) {
            return result;
        }

        let (parent, final_name) = match walk_parent(&self.handle, path, false)? {
            Some(value) => value,
            None => return Ok(ProjectEntry::Absent),
        };
        read_open_node(&parent, &final_name, path)
    }

    pub(crate) fn atomic_write(
        &self,
        path: &SafeRelPath,
        bytes: &[u8],
        replace: bool,
    ) -> Result<(), MaterialsError> {
        let (parent, final_name) =
            walk_parent(&self.handle, path, true)?.ok_or_else(|| path_io_error(path))?;

        if replace {
            match open_node_at(&parent, &final_name) {
                Ok((handle, metadata)) if FileType::from_raw_mode(metadata.st_mode).is_file() => {
                    drop(handle);
                }
                Ok(_) => return Err(commit_error(path)),
                Err(NodeOpenError::Reparse) => return Err(reparse_error(path)),
                Err(NodeOpenError::Missing | NodeOpenError::Io) => {
                    return Err(commit_error(path));
                }
            }
        }

        let (temp_name, mut temp_file) = create_temp_file(&parent, path)?;
        if temp_file.write_all(bytes).is_err() || temp_file.sync_all().is_err() {
            cleanup_exact_temp(&parent, &temp_name, &temp_file);
            return Err(MaterialsError::at_path(
                "materials.apply.write",
                "temporary file could not be written",
                path,
            ));
        }
        if !name_matches_handle(&parent, &temp_name, temp_file.as_fd()) {
            cleanup_exact_temp(&parent, &temp_name, &temp_file);
            return Err(commit_error(path));
        }

        let commit = if replace {
            renameat(&parent, &temp_name, &parent, &final_name)
        } else {
            linkat(&parent, &temp_name, &parent, &final_name, AtFlags::empty())
        };
        if commit.is_err() {
            cleanup_exact_temp(&parent, &temp_name, &temp_file);
            return Err(commit_error(path));
        }
        if !replace {
            cleanup_exact_temp(&parent, &temp_name, &temp_file);
        }
        let _ = fsync(&parent);
        Ok(())
    }

    pub(crate) fn open_managed(
        &self,
        namespace: &SafeRelPath,
    ) -> Result<ManagedDirectory, MaterialsError> {
        let mut current = rio::dup(&self.handle).map_err(|_| path_io_error(namespace))?;
        for component in namespace.components() {
            current = match open_directory_at(&current, component) {
                Ok(directory) => directory,
                Err(DirectoryOpenError::Missing) => return Ok(ManagedDirectory { handle: None }),
                Err(error) => return Err(map_path_directory_error(namespace, error)),
            };
        }
        Ok(ManagedDirectory {
            handle: Some(current),
        })
    }
}

impl ManagedDirectory {
    pub(crate) fn inventory(&self) -> Result<ManagedThirdPartyInventory, MaterialsError> {
        match &self.handle {
            None => Ok(ManagedThirdPartyInventory::absent()),
            Some(handle) => inventory_directory(handle),
        }
    }

    pub(crate) fn ensure_present(&self) -> Result<(), MaterialsError> {
        if self.handle.is_some() {
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
        let handle = self.handle.as_ref().ok_or_else(managed_tree_error)?;
        let (parent, final_name) = match walk_parent(handle, path, false)? {
            Some(value) => value,
            None => return Ok(()),
        };
        let (target, metadata) = match open_node_at(&parent, &final_name) {
            Ok(value) => value,
            Err(NodeOpenError::Missing) => return Ok(()),
            Err(NodeOpenError::Reparse) => return Err(managed_link_error()),
            Err(NodeOpenError::Io) => return Err(managed_remove_error(path)),
        };
        if !FileType::from_raw_mode(metadata.st_mode).is_file()
            || !name_matches_handle(&parent, &final_name, &target)
        {
            return Err(managed_tree_error());
        }
        let mut target = File::from(target);
        if !reader_matches_sha256(&mut target, expected_sha256)
            .map_err(|_| managed_remove_error(path))?
        {
            return Err(managed_changed_error(path));
        }
        if !name_matches_handle(&parent, &final_name, &target) {
            return Err(managed_tree_error());
        }
        unlinkat(&parent, &final_name, AtFlags::empty()).map_err(|_| managed_remove_error(path))
    }

    pub(crate) fn remove_empty_directory(&self, path: &SafeRelPath) -> Result<(), MaterialsError> {
        let handle = self.handle.as_ref().ok_or_else(managed_tree_error)?;
        let (parent, final_name) = match walk_parent(handle, path, false)? {
            Some(value) => value,
            None => return Ok(()),
        };
        let (target, metadata) = match open_node_at(&parent, &final_name) {
            Ok(value) => value,
            Err(NodeOpenError::Missing) => return Ok(()),
            Err(NodeOpenError::Reparse) => return Err(managed_link_error()),
            Err(NodeOpenError::Io) => return Err(managed_remove_error(path)),
        };
        if !FileType::from_raw_mode(metadata.st_mode).is_dir() {
            return Err(managed_tree_error());
        }
        let mut entries = Dir::read_from(&target).map_err(|_| managed_remove_error(path))?;
        while let Some(entry) = entries.read() {
            let entry = entry.map_err(|_| managed_remove_error(path))?;
            let name = OsStr::from_bytes(entry.file_name().to_bytes());
            if !is_dot_entry(name) {
                return Err(managed_tree_error());
            }
        }
        if !name_matches_handle(&parent, &final_name, &target) {
            return Err(managed_tree_error());
        }
        unlinkat(&parent, &final_name, AtFlags::REMOVEDIR).map_err(|_| managed_remove_error(path))
    }
}

fn inventory_directory(root: &OwnedFd) -> Result<ManagedThirdPartyInventory, MaterialsError> {
    let mut state_kind = ManagedEntryKind::Absent;
    let mut staging_kind = ManagedEntryKind::Absent;
    let mut packages = Vec::new();
    let mut extra_root_entries = Vec::new();
    let root_entries = managed_entry_names(root, MAX_MANAGED_ROOT_ENTRIES)?;
    let mut remaining_total = MAX_MANAGED_TOTAL_ENTRIES - root_entries.len();

    for (name, os_name) in root_entries {
        let is_package = validate_managed_package_identity(&name).is_ok();
        if is_package && packages.len() == MAX_MANAGED_PACKAGES {
            return Err(inventory_limit_error());
        }
        let (kind, directory) = inspect_managed_entry(root, &os_name)?;
        match name.as_str() {
            MANAGED_STATE_BASENAME => state_kind = kind,
            MANAGED_STAGING_BASENAME => staging_kind = kind,
            _ if is_package => {
                let mut evidence = Vec::new();
                if let Some(directory) = directory {
                    let evidence_entries = managed_entry_names(
                        &directory,
                        MAX_MANAGED_EVIDENCE_PER_PACKAGE.min(remaining_total),
                    )?;
                    remaining_total -= evidence_entries.len();
                    evidence.reserve(evidence_entries.len());
                    for (basename, os_basename) in evidence_entries {
                        let (entry_kind, _) = inspect_managed_entry(&directory, &os_basename)?;
                        evidence.push(ManagedEvidenceInventory::new(basename, entry_kind));
                    }
                }
                packages.push(ManagedPackageInventory::new(name, kind, evidence));
            }
            _ => extra_root_entries.push(ManagedRootInventoryEntry::new(name, kind)),
        }
    }

    Ok(ManagedThirdPartyInventory::new(
        ManagedEntryKind::Directory,
        state_kind,
        staging_kind,
        packages,
        extra_root_entries,
    ))
}

fn managed_entry_names(
    directory: &OwnedFd,
    max_entries: usize,
) -> Result<Vec<(String, OsString)>, MaterialsError> {
    let mut names = Vec::new();
    let mut entries = Dir::read_from(directory).map_err(|_| inventory_error())?;
    while let Some(entry) = entries.read() {
        let entry = entry.map_err(|_| inventory_error())?;
        let name = OsStr::from_bytes(entry.file_name().to_bytes());
        if is_dot_entry(name) {
            continue;
        }
        if names.len() == max_entries {
            return Err(inventory_limit_error());
        }
        let string = name.to_str().ok_or_else(inventory_error)?.to_owned();
        validate_basename(&string)?;
        names.push((string, name.to_os_string()));
    }
    names.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(names)
}

fn inspect_managed_entry(
    parent: &OwnedFd,
    name: &OsStr,
) -> Result<(ManagedEntryKind, Option<OwnedFd>), MaterialsError> {
    let metadata =
        statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|_| inventory_error())?;
    let file_type = FileType::from_raw_mode(metadata.st_mode);
    if file_type.is_symlink() {
        return Ok((ManagedEntryKind::LinkOrReparsePoint, None));
    }
    if file_type.is_file() {
        return Ok((ManagedEntryKind::File, None));
    }
    if !file_type.is_dir() {
        return Ok((ManagedEntryKind::Other, None));
    }
    match open_directory_at(parent, name) {
        Ok(directory) => Ok((ManagedEntryKind::Directory, Some(directory))),
        Err(DirectoryOpenError::Reparse) => Ok((ManagedEntryKind::LinkOrReparsePoint, None)),
        Err(_) => Err(inventory_error()),
    }
}

fn inventory_error() -> MaterialsError {
    MaterialsError::filesystem(
        "materials.managed.inventory",
        "managed third-party inventory could not be read",
    )
}

fn walk_parent(
    base: &OwnedFd,
    path: &SafeRelPath,
    create: bool,
) -> Result<Option<(OwnedFd, OsString)>, MaterialsError> {
    let components = path.components();
    let final_name = match components.last() {
        Some(name) => name.clone(),
        None => return Err(internal_error()),
    };
    let mut current = rio::dup(base).map_err(|_| path_io_error(path))?;
    for component in &components[..components.len() - 1] {
        current = match open_directory_at(&current, component) {
            Ok(directory) => directory,
            Err(DirectoryOpenError::Missing) if create => {
                match mkdirat(&current, component, DIRECTORY_MODE) {
                    Ok(()) | Err(Errno::EXIST) => {}
                    Err(_) => return Err(path_io_error(path)),
                }
                open_directory_at(&current, component)
                    .map_err(|error| map_path_directory_error(path, error))?
            }
            Err(DirectoryOpenError::Missing) => return Ok(None),
            Err(error) => return Err(map_path_directory_error(path, error)),
        };
    }
    Ok(Some((current, final_name)))
}

fn open_directory_at(parent: &OwnedFd, name: &OsStr) -> Result<OwnedFd, DirectoryOpenError> {
    match openat(parent, name, DIRECTORY_FLAGS, Mode::empty()) {
        Ok(handle) => Ok(handle),
        Err(error) => Err(classify_directory_error(parent, name, error)),
    }
}

fn classify_directory_error(parent: &OwnedFd, name: &OsStr, error: Errno) -> DirectoryOpenError {
    match error {
        Errno::NOENT => DirectoryOpenError::Missing,
        Errno::LOOP => DirectoryOpenError::Reparse,
        Errno::NOTDIR => match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(metadata) if FileType::from_raw_mode(metadata.st_mode).is_symlink() => {
                DirectoryOpenError::Reparse
            }
            Ok(_) => DirectoryOpenError::NotDirectory,
            Err(Errno::NOENT) => DirectoryOpenError::Missing,
            Err(_) => DirectoryOpenError::Io,
        },
        _ => DirectoryOpenError::Io,
    }
}

fn open_node_at(parent: &OwnedFd, name: &OsStr) -> Result<(OwnedFd, Stat), NodeOpenError> {
    let handle = openat(parent, name, FILE_FLAGS, Mode::empty()).map_err(|error| match error {
        Errno::NOENT => NodeOpenError::Missing,
        Errno::LOOP => NodeOpenError::Reparse,
        _ => NodeOpenError::Io,
    })?;
    let metadata = fstat(&handle).map_err(|_| NodeOpenError::Io)?;
    Ok((handle, metadata))
}

fn read_open_node(
    parent: &OwnedFd,
    final_name: &OsStr,
    path: &SafeRelPath,
) -> Result<ProjectEntry, MaterialsError> {
    let (handle, metadata) = match open_node_at(parent, final_name) {
        Ok(value) => value,
        Err(NodeOpenError::Missing) => return Ok(ProjectEntry::Absent),
        Err(NodeOpenError::Reparse) => return Err(reparse_error(path)),
        Err(NodeOpenError::Io) => return Err(path_io_error(path)),
    };
    if !FileType::from_raw_mode(metadata.st_mode).is_file() {
        return Ok(ProjectEntry::Other);
    }
    let mut file = File::from(handle);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|_| path_io_error(path))?;
    Ok(ProjectEntry::File(bytes))
}

#[cfg(target_os = "linux")]
fn read_entry_openat2(
    root: &OwnedFd,
    path: &SafeRelPath,
) -> Option<Result<ProjectEntry, MaterialsError>> {
    use rustix::fs::{ResolveFlags, openat2};

    let resolve = ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS;
    let mut attempts = 0_u8;
    loop {
        match openat2(
            root,
            path.repo_path().as_path(),
            FILE_FLAGS,
            Mode::empty(),
            resolve,
        ) {
            Ok(handle) => {
                let metadata = match fstat(&handle) {
                    Ok(metadata) => metadata,
                    Err(_) => return Some(Err(path_io_error(path))),
                };
                if !FileType::from_raw_mode(metadata.st_mode).is_file() {
                    return Some(Ok(ProjectEntry::Other));
                }
                let mut file = File::from(handle);
                let mut bytes = Vec::new();
                return Some(
                    file.read_to_end(&mut bytes)
                        .map(|_| ProjectEntry::File(bytes))
                        .map_err(|_| path_io_error(path)),
                );
            }
            Err(Errno::AGAIN) if attempts < 4 => attempts += 1,
            Err(Errno::NOSYS | Errno::INVAL) => return None,
            Err(Errno::NOENT) => return Some(Ok(ProjectEntry::Absent)),
            Err(Errno::LOOP | Errno::XDEV) => return Some(Err(reparse_error(path))),
            Err(_) => return Some(Err(path_io_error(path))),
        }
    }
}

fn create_temp_file(
    parent: &OwnedFd,
    path: &SafeRelPath,
) -> Result<(OsString, File), MaterialsError> {
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    for _ in 0..128 {
        let name = temp_component()?;
        match openat(parent, &name, flags, TEMP_MODE) {
            Ok(handle) => return Ok((name, File::from(handle))),
            Err(Errno::EXIST) => {}
            Err(_) => {
                return Err(MaterialsError::at_path(
                    "materials.apply.temp_create",
                    "temporary file could not be created",
                    path,
                ));
            }
        }
    }
    Err(MaterialsError::at_path(
        "materials.apply.temp_create",
        "temporary file could not be created",
        path,
    ))
}

fn cleanup_exact_temp(parent: &OwnedFd, name: &OsStr, file: &File) {
    if name_matches_handle(parent, name, file.as_fd()) {
        let _ = unlinkat(parent, name, AtFlags::empty());
    }
}

fn name_matches_handle(parent: &OwnedFd, name: &OsStr, handle: impl AsFd) -> bool {
    let opened = match fstat(handle) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let named = match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(value) => value,
        Err(_) => return false,
    };
    opened.st_dev == named.st_dev && opened.st_ino == named.st_ino
}

fn map_root_directory_error(error: DirectoryOpenError) -> MaterialsError {
    match error {
        DirectoryOpenError::Reparse => MaterialsError::root(
            "materials.root.reparse",
            "project root cannot contain a symbolic link",
        ),
        DirectoryOpenError::NotDirectory => MaterialsError::root(
            "materials.root.not_directory",
            "project root must be a directory",
        ),
        DirectoryOpenError::Missing | DirectoryOpenError::Io => {
            MaterialsError::root("materials.root.open", "project root cannot be opened")
        }
    }
}

fn map_path_directory_error(path: &SafeRelPath, error: DirectoryOpenError) -> MaterialsError {
    match error {
        DirectoryOpenError::Reparse => reparse_error(path),
        DirectoryOpenError::NotDirectory => MaterialsError::at_path(
            "materials.path.not_directory",
            "path component is not a directory",
            path,
        ),
        DirectoryOpenError::Missing | DirectoryOpenError::Io => path_io_error(path),
    }
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
    MaterialsError::at_path(
        "materials.path.reparse",
        "symbolic links are forbidden",
        path,
    )
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
