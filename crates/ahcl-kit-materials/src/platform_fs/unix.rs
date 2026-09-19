use crate::{MaterialsError, SafeRelPath, is_dot_entry, is_safe_os_component, temp_component};
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
const MAX_REMOVAL_DEPTH: usize = 256;

pub(crate) struct PlatformRoot {
    handle: OwnedFd,
}

pub(crate) struct ManagedDirectory {
    handle: OwnedFd,
}

struct RemovalNode {
    name: OsString,
    handle: OwnedFd,
    metadata: Stat,
    is_directory: bool,
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
    Other,
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
                Err(NodeOpenError::Missing | NodeOpenError::Other | NodeOpenError::Io) => {
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
            current = open_directory_at(&current, component)
                .map_err(|error| map_path_directory_error(namespace, error))?;
        }
        Ok(ManagedDirectory { handle: current })
    }
}

impl ManagedDirectory {
    pub(crate) fn remove_tree(&self, path: &SafeRelPath) -> Result<(), MaterialsError> {
        let (parent, final_name) = match walk_parent(&self.handle, path, false)? {
            Some(value) => value,
            None => return Ok(()),
        };
        let node = match build_removal_node(&parent, &final_name, 0) {
            Ok(node) => node,
            Err(NodeOpenError::Missing) => return Ok(()),
            Err(NodeOpenError::Reparse) => return Err(reparse_error(path)),
            Err(NodeOpenError::Other | NodeOpenError::Io) => return Err(path_io_error(path)),
        };
        delete_removal_node(&parent, node).map_err(|_| {
            MaterialsError::at_path(
                "materials.managed.remove",
                "managed entry could not be removed",
                path,
            )
        })
    }
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
        Err(NodeOpenError::Other | NodeOpenError::Io) => return Err(path_io_error(path)),
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

fn build_removal_node(
    parent: &OwnedFd,
    name: &OsStr,
    depth: usize,
) -> Result<RemovalNode, NodeOpenError> {
    if depth > MAX_REMOVAL_DEPTH {
        return Err(NodeOpenError::Io);
    }
    if !is_safe_os_component(name) {
        return Err(NodeOpenError::Other);
    }

    let named = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| match error {
        Errno::NOENT => NodeOpenError::Missing,
        _ => NodeOpenError::Io,
    })?;
    let file_type = FileType::from_raw_mode(named.st_mode);
    if file_type.is_symlink() {
        return Err(NodeOpenError::Reparse);
    }
    if !file_type.is_file() && !file_type.is_dir() {
        return Err(NodeOpenError::Other);
    }

    let flags = if file_type.is_dir() {
        DIRECTORY_FLAGS
    } else {
        FILE_FLAGS
    };
    let handle = openat(parent, name, flags, Mode::empty()).map_err(|error| match error {
        Errno::NOENT => NodeOpenError::Missing,
        Errno::LOOP => NodeOpenError::Reparse,
        _ => NodeOpenError::Io,
    })?;
    let opened = fstat(&handle).map_err(|_| NodeOpenError::Io)?;
    if opened.st_dev != named.st_dev || opened.st_ino != named.st_ino {
        return Err(NodeOpenError::Io);
    }

    let mut children = Vec::new();
    if file_type.is_dir() {
        let mut directory = Dir::read_from(&handle).map_err(|_| NodeOpenError::Io)?;
        while let Some(entry) = directory.read() {
            let entry = entry.map_err(|_| NodeOpenError::Io)?;
            let child_name = OsStr::from_bytes(entry.file_name().to_bytes());
            if is_dot_entry(child_name) {
                continue;
            }
            children.push(build_removal_node(&handle, child_name, depth + 1)?);
        }
    }

    Ok(RemovalNode {
        name: name.to_os_string(),
        handle,
        metadata: opened,
        is_directory: file_type.is_dir(),
        children,
    })
}

fn delete_removal_node(parent: &OwnedFd, node: RemovalNode) -> rio::Result<()> {
    for child in node.children {
        delete_removal_node(&node.handle, child)?;
    }
    let named = statat(parent, &node.name, AtFlags::SYMLINK_NOFOLLOW)?;
    if named.st_dev != node.metadata.st_dev || named.st_ino != node.metadata.st_ino {
        return Err(Errno::STALE);
    }
    let flags = if node.is_directory {
        AtFlags::REMOVEDIR
    } else {
        AtFlags::empty()
    };
    unlinkat(parent, &node.name, flags)
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
