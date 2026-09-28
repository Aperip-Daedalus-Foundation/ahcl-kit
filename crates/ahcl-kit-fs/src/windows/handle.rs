// crates/ahcl-kit-fs/src/windows/handle.rs - Windows no-follow handle operations.
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

use crate::temp_name::temporary_component;
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io;
use std::mem::{offset_of, size_of};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::ptr;
use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, ERROR_FILE_NOT_FOUND, ERROR_INVALID_PARAMETER,
    ERROR_NOT_SUPPORTED, ERROR_PATH_NOT_FOUND, GENERIC_READ, GENERIC_WRITE, HANDLE,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_INFO_BY_HANDLE_CLASS, FILE_LIST_DIRECTORY, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES,
    FILE_RENAME_INFO_0, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, FileDispositionInfo,
    FileRenameInfo, FileRenameInfoEx, GetFinalPathNameByHandleW, SYNCHRONIZE,
    SetFileInformationByHandle, VOLUME_NAME_DOS,
};

const RENAME_FLAG_REPLACE_IF_EXISTS: u32 = 0x1;
const RENAME_FLAG_POSIX_SEMANTICS: u32 = 0x2;
const MAX_RENAME_UNITS: usize = 32_767;
const FINAL_PATH_FLAGS: u32 = FILE_NAME_NORMALIZED | VOLUME_NAME_DOS;

pub(super) const DIRECTORY_READ_ACCESS: u32 =
    FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE;
pub(super) const OPEN_NO_REPARSE: u32 = FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT;

pub(super) struct DirectoryHandle {
    pub(super) file: File,
    pub(super) final_path: PathBuf,
}

pub(super) struct OpenedNode {
    pub(super) file: File,
    pub(super) attributes: u32,
    pub(super) final_path: Option<PathBuf>,
}

pub(super) enum DirOpen {
    Missing,
    Reparse,
    NotDirectory,
    Io(io::Error),
}

pub(super) enum NodeOpen {
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

pub(super) fn open_directory_path(path: &Path, access: u32) -> Result<DirectoryHandle, DirOpen> {
    let file = open_existing(
        path,
        access,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        OPEN_NO_REPARSE,
    )
    .map_err(classify_directory_error)?;
    let attributes = query_attributes(&file).map_err(DirOpen::Io)?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(DirOpen::Reparse);
    }
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(DirOpen::NotDirectory);
    }
    let final_path = final_path(&file).map_err(DirOpen::Io)?;
    Ok(DirectoryHandle { file, final_path })
}

pub(super) fn open_node_path(path: &Path, access: u32) -> Result<OpenedNode, NodeOpen> {
    open_node_path_with_share(path, access, FILE_SHARE_READ | FILE_SHARE_WRITE)
}

pub(super) fn open_node_path_with_share(
    path: &Path,
    access: u32,
    share_mode: u32,
) -> Result<OpenedNode, NodeOpen> {
    let file =
        open_existing(path, access, share_mode, OPEN_NO_REPARSE).map_err(classify_node_error)?;
    let attributes = query_attributes(&file).map_err(|_| NodeOpen::Io)?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(NodeOpen::Reparse);
    }
    let final_path = directory_final_path(&file, attributes)?;
    Ok(OpenedNode {
        file,
        attributes,
        final_path,
    })
}

fn directory_final_path(file: &File, attributes: u32) -> Result<Option<PathBuf>, NodeOpen> {
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Ok(None);
    }
    final_path(file).map(Some).map_err(|_| NodeOpen::Io)
}

pub(super) fn open_existing(
    path: &Path,
    access: u32,
    share_mode: u32,
    flags: u32,
) -> io::Result<File> {
    OpenOptions::new()
        .access_mode(access)
        .share_mode(share_mode)
        .custom_flags(flags)
        .open(path)
}

fn create_exclusive(path: &Path, access: u32, share_mode: u32, flags: u32) -> io::Result<File> {
    // std rejects `create_new` unless write or append is selected, then `access_mode`
    // replaces that derived mask. `create_new` also sets `FILE_FLAG_OPEN_REPARSE_POINT`.
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .access_mode(access)
        .share_mode(share_mode)
        .custom_flags(flags)
        .open(path)
}

pub(super) fn query_attributes(file: &File) -> io::Result<u32> {
    Ok(file.metadata()?.file_attributes())
}

pub(super) fn final_path(file: &File) -> io::Result<PathBuf> {
    let required = query_final_path_size(file)?;
    let mut buffer = final_path_buffer(required)?;
    let written = write_final_path(file, &mut buffer)?;
    truncate_final_path(buffer, written)
}

fn query_final_path_size(file: &File) -> io::Result<u32> {
    // SAFETY: a zero-length query with a null output pointer requests the required UTF-16
    // buffer size and does not dereference the pointer.
    let required = unsafe {
        GetFinalPathNameByHandleW(file.as_raw_handle(), ptr::null_mut(), 0, FINAL_PATH_FLAGS)
    };
    if required == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(required)
    }
}

fn final_path_buffer(required: u32) -> io::Result<Vec<u16>> {
    let capacity = usize::try_from(required)
        .map_err(|_| io::Error::other("final path is too long"))?
        .saturating_add(1);
    Ok(vec![0_u16; capacity])
}

fn write_final_path(file: &File, buffer: &mut [u16]) -> io::Result<usize> {
    let buffer_len =
        u32::try_from(buffer.len()).map_err(|_| io::Error::other("final path is too long"))?;
    // SAFETY: `buffer` is writable for `buffer_len` UTF-16 units and the borrowed handle
    // remains valid. The API reports the initialized unit count.
    let written = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer_len,
            FINAL_PATH_FLAGS,
        )
    };
    if written == 0 || final_path_overflows(written, buffer.len()) {
        return Err(io::Error::last_os_error());
    }
    usize::try_from(written).map_err(|_| io::Error::other("final path is too long"))
}

fn final_path_overflows(written: u32, len: usize) -> bool {
    usize::try_from(written).map_or(true, |count| count >= len)
}

fn truncate_final_path(mut buffer: Vec<u16>, written: usize) -> io::Result<PathBuf> {
    buffer.truncate(written);
    Ok(PathBuf::from(OsString::from_wide(&buffer)))
}

pub(super) fn create_directory(path: &Path) -> io::Result<()> {
    match std::fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if already_exists(&error) => Ok(()),
        Err(error) => Err(error),
    }
}

pub(super) fn create_temp_file(parent: &DirectoryHandle) -> io::Result<(OsString, File)> {
    for _ in 0..128 {
        if let Some(created) = try_create_temp_file(parent)? {
            return Ok(created);
        }
    }
    Err(io::Error::other("temporary file could not be created"))
}

fn try_create_temp_file(parent: &DirectoryHandle) -> io::Result<Option<(OsString, File)>> {
    let name = temporary_component().map_err(|_| io::Error::other("temporary name unavailable"))?;
    let temp_path = super::path::append_component(&parent.final_path, &name);
    let access = GENERIC_READ | GENERIC_WRITE | DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
    match create_exclusive(
        &temp_path,
        access,
        FILE_SHARE_READ,
        FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
    ) {
        Ok(file) => Ok(Some((name, file))),
        Err(error) if temp_name_exists(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn already_exists(error: &io::Error) -> bool {
    error.raw_os_error() == Some(ERROR_ALREADY_EXISTS as i32)
}

fn temp_name_exists(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(code) if code == ERROR_FILE_EXISTS as i32 || code == ERROR_ALREADY_EXISTS as i32
    )
}

pub(super) fn rename_open_file(
    source: RawHandle,
    target_name: &OsStr,
    replace: bool,
) -> io::Result<()> {
    let wide_name = encode_rename_target(target_name)?;
    let name_bytes = rename_name_bytes(wide_name.len())?;
    let mut buffer = rename_buffer(&wide_name, name_bytes, replace)?;
    let used = rename_record_size(name_bytes)?;
    // The target name comes from a held no-delete-share parent chain, so an ancestor
    // rename cannot invalidate it during the information-class call below.
    commit_rename(source, &mut buffer, used, replace)
}

fn encode_rename_target(target_name: &OsStr) -> io::Result<Vec<u16>> {
    let wide_name = target_name.encode_wide().collect::<Vec<_>>();
    if wide_name.is_empty() || wide_name.len() > MAX_RENAME_UNITS {
        return Err(io::Error::other("target component is too long"));
    }
    Ok(wide_name)
}

fn rename_name_bytes(units: usize) -> io::Result<usize> {
    units
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| io::Error::other("target component is too long"))
}

fn rename_flags(replace: bool) -> FILE_RENAME_INFO_0 {
    if replace {
        FILE_RENAME_INFO_0 {
            Flags: RENAME_FLAG_REPLACE_IF_EXISTS | RENAME_FLAG_POSIX_SEMANTICS,
        }
    } else {
        FILE_RENAME_INFO_0 {
            ReplaceIfExists: false,
        }
    }
}

fn rename_buffer(wide_name: &[u16], name_bytes: usize, replace: bool) -> io::Result<RenameBuffer> {
    let mut buffer = RenameBuffer {
        anonymous: rename_flags(replace),
        root_directory: ptr::null_mut(),
        file_name_length: u32::try_from(name_bytes)
            .map_err(|_| io::Error::other("target component is too long"))?,
        file_name: [0_u16; MAX_RENAME_UNITS],
    };
    buffer.file_name[..wide_name.len()].copy_from_slice(wide_name);
    Ok(buffer)
}

fn rename_record_size(name_bytes: usize) -> io::Result<u32> {
    let used = offset_of!(RenameBuffer, file_name)
        .checked_add(name_bytes)
        .and_then(|value| value.checked_add(size_of::<u16>()))
        .ok_or_else(|| io::Error::other("rename buffer is too large"))?;
    u32::try_from(used).map_err(|_| io::Error::other("rename buffer is too large"))
}

fn commit_rename(
    source: RawHandle,
    buffer: &mut RenameBuffer,
    used: u32,
    replace: bool,
) -> io::Result<()> {
    if !replace {
        return set_rename_information(source, FileRenameInfo, buffer, used);
    }
    // `FileRenameInfoEx` carries the POSIX replace flags. Unsupported hosts fall
    // back to the legacy replace bit without dropping the replace request.
    replace_rename(source, buffer, used)
}

fn replace_rename(source: RawHandle, buffer: &mut RenameBuffer, used: u32) -> io::Result<()> {
    match set_rename_information(source, FileRenameInfoEx, buffer, used) {
        Ok(()) => Ok(()),
        Err(error) if rename_ex_unsupported(&error) => {
            buffer.anonymous = FILE_RENAME_INFO_0 {
                ReplaceIfExists: true,
            };
            set_rename_information(source, FileRenameInfo, buffer, used)
        }
        Err(error) => Err(error),
    }
}

fn rename_ex_unsupported(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(code) if code == ERROR_INVALID_PARAMETER as i32 || code == ERROR_NOT_SUPPORTED as i32
    )
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

pub(super) fn delete_open_handle(handle: RawHandle) -> io::Result<()> {
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

pub(super) fn flush_directory(directory: &File) {
    // Directory durability is best effort. Some filesystems reject flushing a directory handle.
    let _ = directory.sync_all();
}

pub(super) fn directory_is_empty(path: &Path) -> io::Result<bool> {
    std::fs::read_dir(path)?
        .next()
        .transpose()
        .map(|entry| entry.is_none())
}

fn classify_directory_error(error: io::Error) -> DirOpen {
    if missing_name(&error) {
        DirOpen::Missing
    } else {
        DirOpen::Io(error)
    }
}

fn classify_node_error(error: io::Error) -> NodeOpen {
    if missing_name(&error) {
        NodeOpen::Missing
    } else {
        NodeOpen::Io
    }
}

fn missing_name(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(code) if code == ERROR_FILE_NOT_FOUND as i32 || code == ERROR_PATH_NOT_FOUND as i32
    )
}
