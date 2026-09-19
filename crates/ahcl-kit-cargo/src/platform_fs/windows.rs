// crates/ahcl-kit-cargo/src/platform_fs/windows.rs - Windows package evidence capability.
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

use super::{PackageFsError, read_file_with_limit};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::path::{Component, Path, PathBuf, Prefix};
use std::ptr;
use windows_sys::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, GENERIC_READ, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
    FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE,
    FileAttributeTagInfo, GetFileInformationByHandleEx, GetFinalPathNameByHandleW, OPEN_EXISTING,
    SYNCHRONIZE, VOLUME_NAME_DOS,
};

const DIRECTORY_ACCESS: u32 =
    FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE;
const OPEN_NO_REPARSE: u32 = FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT;

pub(crate) struct PackageDirectory {
    chain: Vec<DirectoryHandle>,
}

struct DirectoryHandle {
    _handle: OwnedHandle,
    final_path: PathBuf,
}

struct OpenedNode {
    handle: OwnedHandle,
    attributes: u32,
}

enum DirectoryOpenError {
    Missing,
    Reparse,
    NotDirectory,
    Io(io::Error),
}

impl PackageDirectory {
    pub(crate) fn open(path: &Path) -> Result<Self, PackageFsError> {
        let (volume_root, components) = split_absolute_path(path)?;
        let root = open_directory_path(&volume_root).map_err(map_directory_error)?;
        let mut chain = vec![root];
        for component in components {
            let parent = chain.last().ok_or(PackageFsError::InvalidPath)?;
            let child = open_directory_path(&append_component(&parent.final_path, &component))
                .map_err(map_directory_error)?;
            chain.push(child);
        }
        Ok(Self { chain })
    }

    pub(crate) fn root_license_candidates(
        &self,
        max_files: u64,
    ) -> Result<Vec<PathBuf>, PackageFsError> {
        let root = self.chain.last().ok_or(PackageFsError::InvalidPath)?;
        let mut candidates = Vec::new();
        let mut portable = BTreeSet::new();
        for entry in std::fs::read_dir(&root.final_path).map_err(PackageFsError::Io)? {
            let name = entry.map_err(PackageFsError::Io)?.file_name();
            let name = name.to_str().ok_or(PackageFsError::PathEncoding)?;
            let upper = name.to_ascii_uppercase();
            if !["LICENSE", "COPYING", "NOTICE", "COPYRIGHT"]
                .iter()
                .any(|prefix| upper.starts_with(prefix))
            {
                continue;
            }
            if open_regular_file_path(&append_component(&root.final_path, OsStr::new(name)))?
                .is_none()
            {
                continue;
            }
            if portable.insert(name.to_ascii_lowercase()) {
                let count = u64::try_from(candidates.len())
                    .map_or(u64::MAX, |length| length.saturating_add(1));
                if count > max_files {
                    return Err(PackageFsError::TooManyFiles(count));
                }
                candidates.push(PathBuf::from(name));
            }
        }
        sort_candidates(&mut candidates);
        Ok(candidates)
    }

    pub(crate) fn read_bounded_file(
        &self,
        relative: &Path,
        limit: u64,
    ) -> Result<Option<Vec<u8>>, PackageFsError> {
        let components = normal_components(relative)?;
        let (final_name, parents) = components.split_last().ok_or(PackageFsError::InvalidPath)?;
        let base = self.chain.last().ok_or(PackageFsError::InvalidPath)?;
        let mut directories = Vec::new();
        for component in parents {
            let parent = directories.last().unwrap_or(base);
            let directory = open_directory_path(&append_component(&parent.final_path, component))
                .map_err(map_directory_error)?;
            directories.push(directory);
        }
        let parent = directories.last().unwrap_or(base);
        let Some((file, advertised_len)) =
            open_regular_file_path(&append_component(&parent.final_path, final_name))?
        else {
            return Ok(None);
        };
        read_file_with_limit(file, advertised_len, limit).map(Some)
    }
}

pub(crate) fn read_regular_file(path: &Path) -> Result<Vec<u8>, PackageFsError> {
    let parent = path.parent().ok_or(PackageFsError::InvalidPath)?;
    let name = path.file_name().ok_or(PackageFsError::InvalidPath)?;
    let directory = PackageDirectory::open(parent)?;
    let base = directory.chain.last().ok_or(PackageFsError::InvalidPath)?;
    let Some((file, advertised_len)) =
        open_regular_file_path(&append_component(&base.final_path, name))?
    else {
        return Err(PackageFsError::InvalidPath);
    };
    read_file_with_limit(file, advertised_len, u64::MAX)
}

fn normal_components(path: &Path) -> Result<Vec<OsString>, PackageFsError> {
    if path.as_os_str().is_empty() {
        return Err(PackageFsError::InvalidPath);
    }
    path.components()
        .map(|component| match component {
            Component::Normal(value) => Ok(value.to_os_string()),
            _ => Err(PackageFsError::InvalidPath),
        })
        .collect()
}

fn split_absolute_path(path: &Path) -> Result<(PathBuf, Vec<OsString>), PackageFsError> {
    if !path.is_absolute() {
        return Err(PackageFsError::InvalidPath);
    }

    let mut prefix = None;
    let mut saw_root = false;
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(value) if prefix.is_none() => prefix = Some(value.kind()),
            Component::RootDir if prefix.is_some() && !saw_root => saw_root = true,
            Component::Normal(value) if saw_root => components.push(value.to_os_string()),
            _ => return Err(PackageFsError::InvalidPath),
        }
    }
    if !saw_root {
        return Err(PackageFsError::InvalidPath);
    }
    let root = volume_root_for_prefix(prefix.ok_or(PackageFsError::InvalidPath)?)?;
    Ok((root, components))
}

fn volume_root_for_prefix(prefix: Prefix<'_>) -> Result<PathBuf, PackageFsError> {
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
        _ => return Err(PackageFsError::InvalidPath),
    }
    Ok(PathBuf::from(OsString::from_wide(&wide)))
}

fn open_directory_path(path: &Path) -> Result<DirectoryHandle, DirectoryOpenError> {
    let handle = open_handle(path, DIRECTORY_ACCESS).map_err(classify_directory_error)?;
    let attributes = query_attributes(&handle).map_err(DirectoryOpenError::Io)?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(DirectoryOpenError::Reparse);
    }
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(DirectoryOpenError::NotDirectory);
    }
    let final_path = final_path(&handle).map_err(DirectoryOpenError::Io)?;
    Ok(DirectoryHandle {
        _handle: handle,
        final_path,
    })
}

fn open_node_path(path: &Path) -> Result<OpenedNode, PackageFsError> {
    let handle = open_handle(path, GENERIC_READ | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .map_err(PackageFsError::Io)?;
    let attributes = query_attributes(&handle).map_err(PackageFsError::Io)?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(PackageFsError::LinkOrReparsePoint);
    }
    Ok(OpenedNode { handle, attributes })
}

fn open_regular_file_path(path: &Path) -> Result<Option<(File, u64)>, PackageFsError> {
    let node = open_node_path(path)?;
    if node.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        return Ok(None);
    }
    let file = File::from(node.handle);
    let metadata = file.metadata().map_err(PackageFsError::Io)?;
    if !metadata.is_file() {
        return Ok(None);
    }
    Ok(Some((file, metadata.len())))
}

fn open_handle(path: &Path, access: u32) -> io::Result<OwnedHandle> {
    let wide = wide_null(path.as_os_str())?;
    // SAFETY: `wide` is NUL-terminated and lives through the call. Null security/template
    // pointers are permitted. A non-sentinel return is one newly owned kernel handle.
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            OPEN_NO_REPARSE,
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
    // SAFETY: `information` is writable for `size` bytes and the handle remains live.
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
    // SAFETY: a zero-length query with a null buffer requests the required UTF-16 size.
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
    // SAFETY: `buffer` is writable for `buffer_len` UTF-16 units and the handle remains live.
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

fn classify_directory_error(error: io::Error) -> DirectoryOpenError {
    match error.raw_os_error() {
        Some(code)
            if code == ERROR_FILE_NOT_FOUND as i32 || code == ERROR_PATH_NOT_FOUND as i32 =>
        {
            DirectoryOpenError::Missing
        }
        _ => DirectoryOpenError::Io(error),
    }
}

fn map_directory_error(error: DirectoryOpenError) -> PackageFsError {
    match error {
        DirectoryOpenError::Reparse => PackageFsError::LinkOrReparsePoint,
        DirectoryOpenError::Missing | DirectoryOpenError::NotDirectory => {
            PackageFsError::InvalidPath
        }
        DirectoryOpenError::Io(source) => PackageFsError::Io(source),
    }
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

fn sort_candidates(candidates: &mut [PathBuf]) {
    candidates.sort_by(|left, right| {
        left.to_string_lossy()
            .to_ascii_lowercase()
            .cmp(&right.to_string_lossy().to_ascii_lowercase())
            .then_with(|| left.cmp(right))
    });
}

use std::os::windows::io::AsRawHandle;
