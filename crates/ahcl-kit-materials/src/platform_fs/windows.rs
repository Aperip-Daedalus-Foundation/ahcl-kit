use crate::{MaterialsError, SafeRelPath, is_dot_entry, is_safe_os_component, temp_component};
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
const MAX_REMOVAL_DEPTH: usize = 256;

pub(crate) struct PlatformRoot {
    root_chain: Vec<DirectoryHandle>,
}

pub(crate) struct ManagedDirectory {
    chain: Vec<DirectoryHandle>,
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

struct RemovalNode {
    handle: OwnedHandle,
    children: Vec<RemovalNode>,
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
            let directory = open_directory_path(&path, DIRECTORY_READ_ACCESS)
                .map_err(|error| map_path_directory_error(namespace, error))?;
            chain.push(directory);
        }
        Ok(ManagedDirectory { chain })
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
    pub(crate) fn remove_tree(&self, path: &SafeRelPath) -> Result<(), MaterialsError> {
        let base = current_directory(&self.chain).ok_or_else(internal_path_error)?;
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
            let directory = open_directory_path(&child_path, DIRECTORY_READ_ACCESS)
                .map_err(|error| map_path_directory_error(path, error))?;
            directories.push(directory);
        }
        let parent = select_parent_from_base(base, &directories).ok_or_else(internal_path_error)?;
        let target_path = append_component(&parent.final_path, &final_name);
        let node = match build_removal_node(&target_path, 0) {
            Ok(node) => node,
            Err(NodeOpenError::Missing) => return Ok(()),
            Err(NodeOpenError::Reparse) => return Err(reparse_error(path)),
            Err(NodeOpenError::Io) => return Err(path_io_error(path)),
        };
        delete_removal_node(node).map_err(|_| {
            MaterialsError::at_path(
                "materials.managed.remove",
                "managed entry could not be removed",
                path,
            )
        })
    }
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
    let handle = open_handle(path, access, OPEN_EXISTING, OPEN_NO_REPARSE)
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
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    let size = match u32::try_from(size_of::<FILE_DISPOSITION_INFO>()) {
        Ok(value) => value,
        Err(_) => return,
    };
    // SAFETY: `disposition` is a correctly sized immutable Win32 structure and the caller
    // keeps the handle live. Cleanup is best effort, so failure is intentionally ignored.
    let _ = unsafe {
        SetFileInformationByHandle(
            handle,
            FileDispositionInfo,
            (&raw const disposition).cast(),
            size,
        )
    };
}

fn flush_directory(handle: RawHandle) {
    // SAFETY: the borrowed directory handle remains live. Directory flushing is best effort
    // because some Windows filesystems reject `FlushFileBuffers` for directory handles.
    let _ = unsafe { FlushFileBuffers(handle) };
}

fn build_removal_node(path: &Path, depth: usize) -> Result<RemovalNode, NodeOpenError> {
    if depth > MAX_REMOVAL_DEPTH {
        return Err(NodeOpenError::Io);
    }
    let access = GENERIC_READ | FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | DELETE | SYNCHRONIZE;
    let node = open_node_path(path, access)?;
    let mut children = Vec::new();
    if node.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        let directory_path = match &node.final_path {
            Some(value) => value,
            None => return Err(NodeOpenError::Io),
        };
        let entries = fs_entries(directory_path).map_err(|_| NodeOpenError::Io)?;
        for name in entries {
            if is_dot_entry(&name) {
                continue;
            }
            if !is_safe_os_component(&name) {
                return Err(NodeOpenError::Io);
            }
            let child_path = append_component(directory_path, &name);
            children.push(build_removal_node(&child_path, depth + 1)?);
        }
    }
    Ok(RemovalNode {
        handle: node.handle,
        children,
    })
}

fn fs_entries(path: &Path) -> io::Result<Vec<OsString>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path)? {
        entries.push(entry?.file_name());
    }
    Ok(entries)
}

fn delete_removal_node(node: RemovalNode) -> io::Result<()> {
    for child in node.children {
        delete_removal_node(child)?;
    }
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    let size = u32::try_from(size_of::<FILE_DISPOSITION_INFO>())
        .map_err(|_| io::Error::other("disposition buffer is too large"))?;
    // SAFETY: the disposition structure and size match, and this function owns the live
    // handle until after the call. Children have already been deleted and closed.
    let succeeded = unsafe {
        SetFileInformationByHandle(
            node.handle.as_raw_handle(),
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
