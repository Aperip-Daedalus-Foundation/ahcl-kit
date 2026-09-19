// crates/ahcl-kit-materials/src/platform_fs/windows.rs - Windows capability scoped filesystem backend.
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
    ManagedThirdPartyInventory, MaterialsError, MaterialsErrorCode, SafeRelPath,
    is_safe_os_component, platform_fs::reader_matches_sha256, temp_component,
};
use ahcl_kit_core::ProjectEntry;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::mem::{offset_of, size_of};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Component, Path, PathBuf, Prefix};
use std::ptr;
use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_INVALID_PARAMETER,
    ERROR_NOT_SUPPORTED, ERROR_PATH_NOT_FOUND, GENERIC_READ, GENERIC_WRITE, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateDirectoryW, CreateFileW, DELETE, FILE_ADD_FILE, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
    FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_INFO_BY_HANDLE_CLASS, FILE_LIST_DIRECTORY, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES,
    FILE_RENAME_INFO_0, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, FileAttributeTagInfo,
    FileDispositionInfo, FileRenameInfo, FileRenameInfoEx, FlushFileBuffers,
    GetFileInformationByHandleEx, GetFinalPathNameByHandleW, OPEN_EXISTING, SYNCHRONIZE,
    SetFileInformationByHandle, VOLUME_NAME_DOS,
};

const DIRECTORY_READ_ACCESS: u32 =
    FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE;
const DIRECTORY_WRITE_ACCESS: u32 = DIRECTORY_READ_ACCESS | FILE_ADD_FILE;
const OPEN_NO_REPARSE: u32 = FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT;
const RENAME_FLAG_REPLACE_IF_EXISTS: u32 = 0x1;
const RENAME_FLAG_POSIX_SEMANTICS: u32 = 0x2;
const MAX_RENAME_UNITS: usize = 32_767;

pub(crate) fn fill_random(output: &mut [u8]) -> Result<(), ()> {
    if output.is_empty() {
        return Ok(());
    }
    let length = u32::try_from(output.len()).map_err(|_| ())?;
    // SAFETY: `output` is writable for `length` bytes and remains live through the call.
    // A null algorithm handle is required with `BCRYPT_USE_SYSTEM_PREFERRED_RNG`.
    let status = unsafe {
        BCryptGenRandom(
            ptr::null_mut(),
            output.as_mut_ptr(),
            length,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status >= 0 { Ok(()) } else { Err(()) }
}

pub(crate) struct PlatformRoot {
    root_chain: Vec<DirectoryHandle>,
}

pub(crate) struct ManagedDirectory {
    chain: Option<Vec<DirectoryHandle>>,
}

struct DirectoryHandle {
    handle: OwnedHandle,
    final_path: PathBuf,
}

struct OpenedNode {
    handle: OwnedHandle,
    attributes: u32,
    final_path: Option<PathBuf>,
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

#[repr(C)]
struct RenameBuffer {
    anonymous: FILE_RENAME_INFO_0,
    root_directory: HANDLE,
    file_name_length: u32,
    file_name: [u16; MAX_RENAME_UNITS],
}

impl PlatformRoot {
    pub(crate) fn open(path: &Path) -> Result<Self, MaterialsError> {
        let (volume_root, components) = split_absolute_path(path)?;
        if components.is_empty() {
            return Err(MaterialsError::root(
                "materials.root.filesystem_root",
                "filesystem root cannot be a project root",
            ));
        }

        let root = open_directory_path(&volume_root, DIRECTORY_READ_ACCESS)
            .map_err(|error| map_root_directory_error(error, "project root cannot be opened"))?;
        let mut root_chain = vec![root];
        for component in components {
            let parent_path = current_path(&root_chain).ok_or_else(internal_root_error)?;
            let child_path = append_component(parent_path, &component);
            let child =
                open_directory_path(&child_path, DIRECTORY_READ_ACCESS).map_err(|error| {
                    map_root_directory_error(error, "project root cannot be opened")
                })?;
            root_chain.push(child);
        }
        Ok(Self { root_chain })
    }

    pub(crate) fn read_entry(&self, path: &SafeRelPath) -> Result<ProjectEntry, MaterialsError> {
        let (directories, final_name) =
            match self.walk_parent(path, false, DIRECTORY_READ_ACCESS)? {
                Some(value) => value,
                None => return Ok(ProjectEntry::Absent),
            };
        let parent =
            select_parent(&self.root_chain, &directories).ok_or_else(internal_path_error)?;
        let node_path = append_component(&parent.final_path, &final_name);
        let node = match open_node_path(
            &node_path,
            GENERIC_READ | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        ) {
            Ok(node) => node,
            Err(NodeOpenError::Missing) => return Ok(ProjectEntry::Absent),
            Err(NodeOpenError::Reparse) => return Err(reparse_error(path)),
            Err(NodeOpenError::Io) => return Err(path_io_error(path)),
        };
        if node.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            return Ok(ProjectEntry::Other);
        }

        let mut bytes = Vec::new();
        let mut file = File::from(node.handle);
        file.read_to_end(&mut bytes)
            .map_err(|_| path_io_error(path))?;
        Ok(ProjectEntry::File(bytes))
    }

    pub(crate) fn atomic_write(
        &self,
        path: &SafeRelPath,
        bytes: &[u8],
        replace: bool,
    ) -> Result<(), MaterialsError> {
        let operation_root = self.open_operation_root(DIRECTORY_WRITE_ACCESS, path)?;
        let (directories, final_name) = self
            .walk_parent_from(&operation_root, path, true, DIRECTORY_WRITE_ACCESS)?
            .ok_or_else(|| path_io_error(path))?;
        let parent = select_parent_from_base(&operation_root, &directories)
            .ok_or_else(internal_path_error)?;

        if replace {
            let target_path = append_component(&parent.final_path, &final_name);
            match open_node_path(&target_path, FILE_READ_ATTRIBUTES | SYNCHRONIZE) {
                Ok(node) if node.attributes & FILE_ATTRIBUTE_DIRECTORY == 0 => {}
                Ok(_) => return Err(commit_error(path)),
                Err(NodeOpenError::Reparse) => return Err(reparse_error(path)),
                Err(NodeOpenError::Missing | NodeOpenError::Io) => return Err(commit_error(path)),
            }
        }

        let (temp_name, mut temp_file) = create_temp_file(parent, path)?;
        if temp_file.write_all(bytes).is_err() || temp_file.sync_all().is_err() {
            mark_delete(temp_file.as_raw_handle());
            return Err(MaterialsError::at_path(
                "materials.apply.write",
                "temporary file could not be written",
                path,
            ));
        }

        let target_path = append_component(&parent.final_path, &final_name);
        if rename_open_file(temp_file.as_raw_handle(), target_path.as_os_str(), replace).is_err() {
            mark_delete(temp_file.as_raw_handle());
            return Err(commit_error(path));
        }

        let _ = temp_name;
        flush_directory(parent.handle.as_raw_handle());
        Ok(())
    }

    pub(crate) fn open_managed(
        &self,
        namespace: &SafeRelPath,
    ) -> Result<ManagedDirectory, MaterialsError> {
        let mut chain = Vec::new();
        for component in namespace.components() {
            let parent = select_parent(&self.root_chain, &chain).ok_or_else(internal_path_error)?;
            let path = append_component(&parent.final_path, component);
            let directory = match open_directory_path(&path, DIRECTORY_READ_ACCESS) {
                Ok(directory) => directory,
                Err(DirectoryOpenError::Missing) => {
                    return Ok(ManagedDirectory { chain: None });
                }
                Err(error) => return Err(map_path_directory_error(namespace, error)),
            };
            chain.push(directory);
        }
        Ok(ManagedDirectory { chain: Some(chain) })
    }

    fn open_operation_root(
        &self,
        access: u32,
        path: &SafeRelPath,
    ) -> Result<DirectoryHandle, MaterialsError> {
        let current = current_directory(&self.root_chain).ok_or_else(internal_path_error)?;
        open_directory_path(&current.final_path, access)
            .map_err(|error| map_path_directory_error(path, error))
    }

    fn walk_parent(
        &self,
        path: &SafeRelPath,
        create: bool,
        access: u32,
    ) -> Result<Option<(Vec<DirectoryHandle>, OsString)>, MaterialsError> {
        let base = current_directory(&self.root_chain).ok_or_else(internal_path_error)?;
        self.walk_parent_from(base, path, create, access)
    }

    fn walk_parent_from(
        &self,
        base: &DirectoryHandle,
        path: &SafeRelPath,
        create: bool,
        access: u32,
    ) -> Result<Option<(Vec<DirectoryHandle>, OsString)>, MaterialsError> {
        let components = path.components();
        let final_name = match components.last() {
            Some(name) => name.clone(),
            None => return Err(internal_path_error()),
        };
        let mut directories = Vec::new();
        for component in &components[..components.len() - 1] {
            let parent =
                select_parent_from_base(base, &directories).ok_or_else(internal_path_error)?;
            let child_path = append_component(&parent.final_path, component);
            let opened = match open_directory_path(&child_path, access) {
                Ok(directory) => directory,
                Err(DirectoryOpenError::Missing) if create => {
                    create_directory(&child_path).map_err(|_| path_io_error(path))?;
                    open_directory_path(&child_path, access)
                        .map_err(|error| map_path_directory_error(path, error))?
                }
                Err(DirectoryOpenError::Missing) => return Ok(None),
                Err(error) => return Err(map_path_directory_error(path, error)),
            };
            directories.push(opened);
        }
        Ok(Some((directories, final_name)))
    }
}

impl ManagedDirectory {
    pub(crate) fn inventory(&self) -> Result<ManagedThirdPartyInventory, MaterialsError> {
        match &self.chain {
            None => Ok(ManagedThirdPartyInventory::absent()),
            Some(chain) => inventory_directory(chain),
        }
    }

    pub(crate) fn ensure_present(&self) -> Result<(), MaterialsError> {
        if self.chain.is_some() {
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
        let chain = self.chain.as_ref().ok_or_else(managed_tree_error)?;
        let Some((directories, final_name)) = walk_managed_parent(chain, path)? else {
            return Ok(());
        };
        let base = current_directory(chain).ok_or_else(internal_path_error)?;
        let parent = select_parent_from_base(base, &directories).ok_or_else(internal_path_error)?;
        let target_path = append_component(&parent.final_path, &final_name);
        let node = match open_node_path_with_share(
            &target_path,
            DELETE | GENERIC_READ | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_READ,
        ) {
            Ok(node) => node,
            Err(NodeOpenError::Missing) => return Ok(()),
            Err(NodeOpenError::Reparse) => return Err(managed_link_error()),
            Err(NodeOpenError::Io) => return Err(managed_remove_error(path)),
        };
        if node.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            return Err(managed_tree_error());
        }
        let mut target = File::from(node.handle);
        if !reader_matches_sha256(&mut target, expected_sha256)
            .map_err(|_| managed_remove_error(path))?
        {
            return Err(managed_changed_error(path));
        }
        delete_open_handle(target.as_raw_handle()).map_err(|_| managed_remove_error(path))
    }

    pub(crate) fn remove_empty_directory(&self, path: &SafeRelPath) -> Result<(), MaterialsError> {
        let chain = self.chain.as_ref().ok_or_else(managed_tree_error)?;
        let Some((directories, final_name)) = walk_managed_parent(chain, path)? else {
            return Ok(());
        };
        let base = current_directory(chain).ok_or_else(internal_path_error)?;
        let parent = select_parent_from_base(base, &directories).ok_or_else(internal_path_error)?;
        let target_path = append_component(&parent.final_path, &final_name);
        let node = match open_node_path(
            &target_path,
            DELETE | FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        ) {
            Ok(node) => node,
            Err(NodeOpenError::Missing) => return Ok(()),
            Err(NodeOpenError::Reparse) => return Err(managed_link_error()),
            Err(NodeOpenError::Io) => return Err(managed_remove_error(path)),
        };
        if node.attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
            return Err(managed_tree_error());
        }
        let directory_path = node.final_path.as_ref().ok_or_else(managed_tree_error)?;
        if !directory_is_empty(directory_path).map_err(|_| managed_remove_error(path))? {
            return Err(managed_tree_error());
        }
        delete_open_handle(node.handle.as_raw_handle()).map_err(|_| managed_remove_error(path))
    }
}

fn walk_managed_parent(
    chain: &[DirectoryHandle],
    path: &SafeRelPath,
) -> Result<Option<(Vec<DirectoryHandle>, OsString)>, MaterialsError> {
    let base = current_directory(chain).ok_or_else(internal_path_error)?;
    let components = path.components();
    let final_name = components.last().cloned().ok_or_else(internal_path_error)?;
    let mut directories = Vec::new();
    for component in &components[..components.len() - 1] {
        let parent = select_parent_from_base(base, &directories).ok_or_else(internal_path_error)?;
        let child_path = append_component(&parent.final_path, component);
        let directory = match open_directory_path(&child_path, DIRECTORY_READ_ACCESS) {
            Ok(directory) => directory,
            Err(DirectoryOpenError::Missing) => return Ok(None),
            Err(DirectoryOpenError::Reparse) => return Err(managed_link_error()),
            Err(DirectoryOpenError::NotDirectory) => return Err(managed_tree_error()),
            Err(DirectoryOpenError::Io) => return Err(managed_remove_error(path)),
        };
        directories.push(directory);
    }
    Ok(Some((directories, final_name)))
}

fn inventory_directory(
    chain: &[DirectoryHandle],
) -> Result<ManagedThirdPartyInventory, MaterialsError> {
    let root = current_directory(chain).ok_or_else(inventory_error)?;
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
    directory: &DirectoryHandle,
    max_entries: usize,
) -> Result<Vec<(String, OsString)>, MaterialsError> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&directory.final_path).map_err(|_| inventory_error())? {
        if names.len() == max_entries {
            return Err(inventory_limit_error());
        }
        let name = entry.map_err(|_| inventory_error())?.file_name();
        let string = name.to_str().ok_or_else(inventory_error)?.to_owned();
        validate_basename(&string)?;
        names.push((string, name));
    }
    names.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(names)
}

fn inspect_managed_entry(
    parent: &DirectoryHandle,
    name: &OsStr,
) -> Result<(ManagedEntryKind, Option<DirectoryHandle>), MaterialsError> {
    let path = append_component(&parent.final_path, name);
    let handle = open_handle(
        &path,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        OPEN_EXISTING,
        OPEN_NO_REPARSE,
    )
    .map_err(|_| inventory_error())?;
    let attributes = query_attributes(&handle).map_err(|_| inventory_error())?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Ok((ManagedEntryKind::LinkOrReparsePoint, None));
    }
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Ok((ManagedEntryKind::File, None));
    }
    let final_path = final_path(&handle).map_err(|_| inventory_error())?;
    Ok((
        ManagedEntryKind::Directory,
        Some(DirectoryHandle { handle, final_path }),
    ))
}

fn inventory_error() -> MaterialsError {
    MaterialsError::filesystem(
        "materials.managed.inventory",
        "managed third-party inventory could not be read",
    )
}

fn split_absolute_path(path: &Path) -> Result<(PathBuf, Vec<OsString>), MaterialsError> {
    if !path.is_absolute() {
        return Err(MaterialsError::root(
            "materials.root.not_absolute",
            "project root must be absolute",
        ));
    }

    let mut prefix = None;
    let mut saw_root = false;
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(value) if prefix.is_none() => prefix = Some(value.kind()),
            Component::RootDir if prefix.is_some() && !saw_root => saw_root = true,
            Component::Normal(value) if saw_root && is_safe_os_component(value) => {
                components.push(value.to_os_string());
            }
            _ => return Err(invalid_root_error()),
        }
    }
    if !saw_root {
        return Err(invalid_root_error());
    }
    let prefix = prefix.ok_or_else(invalid_root_error)?;
    let root = volume_root_for_prefix(prefix)?;
    Ok((root, components))
}

fn volume_root_for_prefix(prefix: Prefix<'_>) -> Result<PathBuf, MaterialsError> {
    let mut wide = r"\\?\".encode_utf16().collect::<Vec<_>>();
    match prefix {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
            wide.push(u16::from(letter));
            wide.push(u16::from(b':'));
            wide.push(u16::from(b'\\'));
        }
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
            wide.extend("UNC\\".encode_utf16());
            wide.extend(server.encode_wide());
            wide.push(u16::from(b'\\'));
            wide.extend(share.encode_wide());
            wide.push(u16::from(b'\\'));
        }
        _ => return Err(invalid_root_error()),
    }
    Ok(PathBuf::from(OsString::from_wide(&wide)))
}

fn open_directory_path(path: &Path, access: u32) -> Result<DirectoryHandle, DirectoryOpenError> {
    let handle = open_handle(path, access, OPEN_EXISTING, OPEN_NO_REPARSE)
        .map_err(classify_directory_open_error)?;
    let attributes = query_attributes(&handle).map_err(|_| DirectoryOpenError::Io)?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(DirectoryOpenError::Reparse);
    }
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(DirectoryOpenError::NotDirectory);
    }
    let final_path = final_path(&handle).map_err(|_| DirectoryOpenError::Io)?;
    Ok(DirectoryHandle { handle, final_path })
}

fn open_node_path(path: &Path, access: u32) -> Result<OpenedNode, NodeOpenError> {
    open_node_path_with_share(path, access, FILE_SHARE_READ | FILE_SHARE_WRITE)
}

fn open_node_path_with_share(
    path: &Path,
    access: u32,
    share_mode: u32,
) -> Result<OpenedNode, NodeOpenError> {
    let handle = open_handle_with_share(path, access, share_mode, OPEN_EXISTING, OPEN_NO_REPARSE)
        .map_err(classify_node_open_error)?;
    let attributes = query_attributes(&handle).map_err(|_| NodeOpenError::Io)?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(NodeOpenError::Reparse);
    }
    let final_path = if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        Some(final_path(&handle).map_err(|_| NodeOpenError::Io)?)
    } else {
        None
    };
    Ok(OpenedNode {
        handle,
        attributes,
        final_path,
    })
}

fn open_handle(path: &Path, access: u32, disposition: u32, flags: u32) -> io::Result<OwnedHandle> {
    open_handle_with_share(
        path,
        access,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        disposition,
        flags,
    )
}

fn open_handle_with_share(
    path: &Path,
    access: u32,
    share_mode: u32,
    disposition: u32,
    flags: u32,
) -> io::Result<OwnedHandle> {
    let wide = wide_null(path.as_os_str())?;
    // SAFETY: `wide` is NUL-terminated and lives through the call. Null security/template
    // pointers are permitted. A non-sentinel return is one newly owned kernel handle.
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            access,
            share_mode,
            ptr::null(),
            disposition,
            flags,
            ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful `CreateFileW` returned a unique owned handle and ownership is
    // transferred exactly once to `OwnedHandle`.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

fn query_attributes(handle: &OwnedHandle) -> io::Result<u32> {
    let mut information = FILE_ATTRIBUTE_TAG_INFO::default();
    let size = u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>())
        .map_err(|_| io::Error::other("attribute buffer is too large"))?;
    // SAFETY: `information` is a writable buffer of the exact advertised size and the
    // borrowed handle remains valid for the duration of the call.
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle(),
            FileAttributeTagInfo,
            (&raw mut information).cast(),
            size,
        )
    };
    if succeeded == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(information.FileAttributes)
    }
}

fn final_path(handle: &OwnedHandle) -> io::Result<PathBuf> {
    // SAFETY: a zero-length query with a null output pointer requests the required UTF-16
    // buffer size and does not dereference the pointer.
    let required = unsafe {
        GetFinalPathNameByHandleW(
            handle.as_raw_handle(),
            ptr::null_mut(),
            0,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    if required == 0 {
        return Err(io::Error::last_os_error());
    }
    let capacity = usize::try_from(required)
        .map_err(|_| io::Error::other("final path is too long"))?
        .saturating_add(1);
    let mut buffer = vec![0_u16; capacity];
    let buffer_len =
        u32::try_from(buffer.len()).map_err(|_| io::Error::other("final path is too long"))?;
    // SAFETY: `buffer` is writable for `buffer_len` UTF-16 units and the borrowed handle
    // remains valid. The API reports the initialized unit count.
    let written = unsafe {
        GetFinalPathNameByHandleW(
            handle.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer_len,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    if written == 0 || usize::try_from(written).map_or(true, |count| count >= buffer.len()) {
        return Err(io::Error::last_os_error());
    }
    let written =
        usize::try_from(written).map_err(|_| io::Error::other("final path is too long"))?;
    buffer.truncate(written);
    Ok(PathBuf::from(OsString::from_wide(&buffer)))
}

fn create_directory(path: &Path) -> io::Result<()> {
    let wide = wide_null(path.as_os_str())?;
    // SAFETY: `wide` is NUL-terminated and the null security pointer requests defaults.
    let succeeded = unsafe { CreateDirectoryW(wide.as_ptr(), ptr::null()) };
    if succeeded != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if matches!(error.raw_os_error(), Some(code) if code == ERROR_ALREADY_EXISTS as i32) {
        Ok(())
    } else {
        Err(error)
    }
}

fn create_temp_file(
    parent: &DirectoryHandle,
    path: &SafeRelPath,
) -> Result<(OsString, File), MaterialsError> {
    for _ in 0..128 {
        let name = temp_component()?;
        let temp_path = append_component(&parent.final_path, &name);
        let access = GENERIC_READ | GENERIC_WRITE | DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
        match open_handle_with_share(
            &temp_path,
            access,
            FILE_SHARE_READ,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
        ) {
            Ok(handle) => return Ok((name, File::from(handle))),
            Err(error)
                if matches!(
                    error.raw_os_error(),
                    Some(code) if code == ERROR_FILE_EXISTS as i32 || code == ERROR_ALREADY_EXISTS as i32
                ) => {}
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

fn rename_open_file(source: RawHandle, target_name: &OsStr, replace: bool) -> io::Result<()> {
    let wide_name = target_name.encode_wide().collect::<Vec<_>>();
    if wide_name.is_empty() || wide_name.len() > MAX_RENAME_UNITS {
        return Err(io::Error::other("target component is too long"));
    }
    let name_bytes = wide_name
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| io::Error::other("target component is too long"))?;
    let anonymous = if replace {
        FILE_RENAME_INFO_0 {
            Flags: RENAME_FLAG_REPLACE_IF_EXISTS | RENAME_FLAG_POSIX_SEMANTICS,
        }
    } else {
        FILE_RENAME_INFO_0 {
            ReplaceIfExists: false,
        }
    };
    let mut buffer = RenameBuffer {
        anonymous,
        root_directory: ptr::null_mut(),
        file_name_length: u32::try_from(name_bytes)
            .map_err(|_| io::Error::other("target component is too long"))?,
        file_name: [0_u16; MAX_RENAME_UNITS],
    };
    buffer.file_name[..wide_name.len()].copy_from_slice(&wide_name);
    let used = offset_of!(RenameBuffer, file_name)
        .checked_add(name_bytes)
        .and_then(|value| value.checked_add(size_of::<u16>()))
        .ok_or_else(|| io::Error::other("rename buffer is too large"))?;
    let used = u32::try_from(used).map_err(|_| io::Error::other("rename buffer is too large"))?;
    // SAFETY: `RenameBuffer` is `repr(C)` with the Win32 `FILE_RENAME_INFO` prefix and
    // `file_name_length` describes initialized UTF-16 units in its trailing array. The source
    // handle stays live, while the full target name was derived from a held no-delete-share
    // parent chain and cannot be invalidated by an ancestor rename during this call.
    if !replace {
        return set_rename_information(source, FileRenameInfo, &buffer, used);
    }

    match set_rename_information(source, FileRenameInfoEx, &buffer, used) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.raw_os_error(),
                Some(code)
                    if code == ERROR_INVALID_PARAMETER as i32
                        || code == ERROR_NOT_SUPPORTED as i32
            ) =>
        {
            buffer.anonymous = FILE_RENAME_INFO_0 {
                ReplaceIfExists: true,
            };
            set_rename_information(source, FileRenameInfo, &buffer, used)
        }
        Err(error) => Err(error),
    }
}

fn set_rename_information(
    source: RawHandle,
    information_class: FILE_INFO_BY_HANDLE_CLASS,
    buffer: &RenameBuffer,
    used: u32,
) -> io::Result<()> {
    // SAFETY: `buffer` has the Win32 `FILE_RENAME_INFO` prefix, `used` covers its initialized
    // variable-length name, and the source handle remains live throughout the call.
    let succeeded = unsafe {
        SetFileInformationByHandle(
            source,
            information_class,
            (buffer as *const RenameBuffer).cast(),
            used,
        )
    };
    if succeeded == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn mark_delete(handle: RawHandle) {
    let _ = delete_open_handle(handle);
}

fn delete_open_handle(handle: RawHandle) -> io::Result<()> {
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    let size = u32::try_from(size_of::<FILE_DISPOSITION_INFO>())
        .map_err(|_| io::Error::other("disposition buffer is too large"))?;
    // SAFETY: `disposition` is a correctly sized immutable Win32 structure and the caller keeps
    // the handle live through the call.
    let succeeded = unsafe {
        SetFileInformationByHandle(
            handle,
            FileDispositionInfo,
            (&raw const disposition).cast(),
            size,
        )
    };
    if succeeded == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn flush_directory(handle: RawHandle) {
    // SAFETY: the borrowed directory handle remains live. Directory flushing is best effort
    // because some Windows filesystems reject `FlushFileBuffers` for directory handles.
    let _ = unsafe { FlushFileBuffers(handle) };
}

fn directory_is_empty(path: &Path) -> io::Result<bool> {
    std::fs::read_dir(path)?
        .next()
        .transpose()
        .map(|entry| entry.is_none())
}

fn wide_null(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut wide = value.encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "embedded NUL"));
    }
    wide.push(0);
    Ok(wide)
}

fn append_component(parent: &Path, component: &OsStr) -> PathBuf {
    let mut path = parent.to_path_buf();
    path.push(component);
    path
}

fn current_directory(chain: &[DirectoryHandle]) -> Option<&DirectoryHandle> {
    chain.last()
}

fn current_path(chain: &[DirectoryHandle]) -> Option<&Path> {
    current_directory(chain).map(|directory| directory.final_path.as_path())
}

fn select_parent<'a>(
    root_chain: &'a [DirectoryHandle],
    directories: &'a [DirectoryHandle],
) -> Option<&'a DirectoryHandle> {
    match directories.last() {
        Some(directory) => Some(directory),
        None => root_chain.last(),
    }
}

fn select_parent_from_base<'a>(
    base: &'a DirectoryHandle,
    directories: &'a [DirectoryHandle],
) -> Option<&'a DirectoryHandle> {
    match directories.last() {
        Some(directory) => Some(directory),
        None => Some(base),
    }
}

fn classify_directory_open_error(error: io::Error) -> DirectoryOpenError {
    match error.raw_os_error() {
        Some(code)
            if code == ERROR_FILE_NOT_FOUND as i32 || code == ERROR_PATH_NOT_FOUND as i32 =>
        {
            DirectoryOpenError::Missing
        }
        _ => DirectoryOpenError::Io,
    }
}

fn classify_node_open_error(error: io::Error) -> NodeOpenError {
    match error.raw_os_error() {
        Some(code)
            if code == ERROR_FILE_NOT_FOUND as i32 || code == ERROR_PATH_NOT_FOUND as i32 =>
        {
            NodeOpenError::Missing
        }
        _ => NodeOpenError::Io,
    }
}

fn map_root_directory_error(error: DirectoryOpenError, message: &'static str) -> MaterialsError {
    match error {
        DirectoryOpenError::Reparse => MaterialsError::root(
            "materials.root.reparse",
            "project root cannot contain a link or reparse point",
        ),
        DirectoryOpenError::NotDirectory => MaterialsError::root(
            "materials.root.not_directory",
            "project root must be a directory",
        ),
        DirectoryOpenError::Missing | DirectoryOpenError::Io => {
            MaterialsError::root("materials.root.open", message)
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

fn internal_root_error() -> MaterialsError {
    MaterialsError::root(
        "materials.internal.invariant",
        "filesystem capability invariant failed",
    )
}

fn internal_path_error() -> MaterialsError {
    MaterialsError::root(
        "materials.internal.invariant",
        "filesystem capability invariant failed",
    )
}

fn reparse_error(path: &SafeRelPath) -> MaterialsError {
    MaterialsError::at_path(
        "materials.path.reparse",
        "links and reparse points are forbidden",
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
