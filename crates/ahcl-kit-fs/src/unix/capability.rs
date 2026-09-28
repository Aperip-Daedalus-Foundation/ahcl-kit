// crates/ahcl-kit-fs/src/unix/capability.rs - Unix no-follow project tree reads and writes.
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

use crate::failure::{
    Inspected, PathFailure, ReadNode, RemoveFailure, RemoveStatus, RootFailure, ScanFailure,
    WriteFailure,
};
use crate::temp_name::temporary_component;
use rustix::fd::{AsFd, OwnedFd};
use rustix::fs::{
    AtFlags, Dir, FileType, Mode, OFlags, Stat, fstat, fsync, linkat, mkdirat, open, openat,
    renameat, statat, unlinkat,
};
use rustix::io::{self as rio, Errno};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
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
const TEMP_FLAGS: OFlags = OFlags::WRONLY
    .union(OFlags::CREATE)
    .union(OFlags::EXCL)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

pub struct FsRoot {
    handle: OwnedFd,
}

pub struct FsDirectory {
    handle: OwnedFd,
}

enum DirectoryOpen {
    Missing,
    Reparse,
    NotDirectory,
    Io,
}

enum NodeOpen {
    Missing,
    Reparse,
    Io,
}

enum RootPiece {
    Root,
    Normal(OsString),
}

impl FsRoot {
    pub fn open(path: &Path) -> Result<Self, RootFailure> {
        let components = project_components(path)?;
        let mut current = open_unix_root()?;
        for component in components {
            current = open_directory_at(&current, &component).map_err(map_root_directory)?;
        }
        Ok(Self { handle: current })
    }

    pub fn read_entry(
        &self,
        relative: &Path,
        components: &[OsString],
    ) -> Result<ReadNode, PathFailure> {
        // Linux can resolve the whole relative path beneath this root before
        // the component walk. Other Unix targets use no-follow opens only.
        #[cfg(target_os = "linux")]
        if let Some(result) = read_entry_openat2(&self.handle, relative) {
            return result;
        }
        let _ = relative;
        match walk_parent(&self.handle, components, false)? {
            Some((parent, name)) => read_open_node(&parent, &name),
            None => Ok(ReadNode::Absent),
        }
    }

    pub fn write_file(
        &self,
        components: &[OsString],
        bytes: &[u8],
        replace: bool,
    ) -> Result<(), WriteFailure> {
        let (parent, final_name) = writable_parent(&self.handle, components)?;
        if replace {
            require_replace_file(&parent, &final_name)?;
        }
        commit_new_file(&parent, &final_name, bytes, replace)
    }

    pub fn open_directory(
        &self,
        components: &[OsString],
    ) -> Result<Option<FsDirectory>, PathFailure> {
        if components.is_empty() {
            return duplicate_directory(&self.handle);
        }
        let Some((parent, name)) = walk_parent(&self.handle, components, false)? else {
            return Ok(None);
        };
        open_final_directory(&parent, &name)
    }
}

impl FsDirectory {
    pub fn scan_names<E, F>(&self, max_entries: usize, mut visit: F) -> Result<(), ScanFailure<E>>
    where
        F: FnMut(&OsStr) -> Result<(), E>,
    {
        let mut entries = Dir::read_from(&self.handle).map_err(|_| ScanFailure::Io)?;
        let mut seen = 0_usize;
        while let Some(entry) = entries.read() {
            let entry = entry.map_err(|_| ScanFailure::Io)?;
            seen = visit_listed(entry_name(&entry), seen, max_entries, &mut visit)?;
        }
        Ok(())
    }

    pub fn inspect(&self, name: &OsStr) -> Result<Inspected<FsDirectory>, ScanFailure<()>> {
        let metadata =
            statat(&self.handle, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|_| ScanFailure::Io)?;
        classify_stat(&self.handle, name, &metadata)
    }

    pub fn remove_file<F>(
        &self,
        components: &[OsString],
        accept: F,
    ) -> Result<RemoveStatus, RemoveFailure>
    where
        F: FnOnce(&mut File) -> io::Result<bool>,
    {
        let Some((parent, name)) = walk_for_remove(&self.handle, components)? else {
            return Ok(RemoveStatus::Absent);
        };
        remove_opened_file(&parent, &name, accept)
    }

    pub fn remove_empty_directory(
        &self,
        components: &[OsString],
    ) -> Result<RemoveStatus, RemoveFailure> {
        let Some((parent, name)) = walk_for_remove(&self.handle, components)? else {
            return Ok(RemoveStatus::Absent);
        };
        let Some(directory) = open_dir_for_remove(&parent, &name)? else {
            return Ok(RemoveStatus::Absent);
        };
        remove_if_empty(&parent, &name, directory)
    }
}

fn project_components(path: &Path) -> Result<Vec<OsString>, RootFailure> {
    if !path.is_absolute() {
        return Err(RootFailure::NotAbsolute);
    }
    let components = collect_project_components(path)?;
    if components.is_empty() {
        Err(RootFailure::FilesystemRoot)
    } else {
        Ok(components)
    }
}

fn collect_project_components(path: &Path) -> Result<Vec<OsString>, RootFailure> {
    let mut components = Vec::new();
    let mut saw_root = false;
    for component in path.components() {
        match root_piece(component, saw_root)? {
            RootPiece::Root => saw_root = true,
            RootPiece::Normal(value) => components.push(value),
        }
    }
    if saw_root {
        Ok(components)
    } else {
        Err(RootFailure::Invalid)
    }
}

fn root_piece(component: Component<'_>, saw_root: bool) -> Result<RootPiece, RootFailure> {
    match component {
        Component::RootDir if !saw_root => Ok(RootPiece::Root),
        Component::Normal(value) if saw_root => Ok(RootPiece::Normal(value.to_os_string())),
        _ => Err(RootFailure::Invalid),
    }
}

fn open_unix_root() -> Result<OwnedFd, RootFailure> {
    open(Path::new("/"), DIRECTORY_FLAGS, Mode::empty()).map_err(|_| RootFailure::Open)
}

fn map_root_directory(error: DirectoryOpen) -> RootFailure {
    match error {
        DirectoryOpen::Reparse => RootFailure::Reparse,
        DirectoryOpen::NotDirectory => RootFailure::NotDirectory,
        DirectoryOpen::Missing | DirectoryOpen::Io => RootFailure::Open,
    }
}

fn writable_parent(
    root: &OwnedFd,
    components: &[OsString],
) -> Result<(OwnedFd, OsString), WriteFailure> {
    match walk_parent(root, components, true) {
        Ok(Some(parent)) => Ok(parent),
        Ok(None) => Err(WriteFailure::Path(PathFailure::Io)),
        Err(error) => Err(WriteFailure::Path(error)),
    }
}

fn require_replace_file(parent: &OwnedFd, name: &OsStr) -> Result<(), WriteFailure> {
    match open_node_at(parent, name) {
        Ok((_, metadata)) if file_type(&metadata).is_file() => Ok(()),
        Ok(_) => Err(WriteFailure::Commit),
        Err(NodeOpen::Reparse) => Err(WriteFailure::Path(PathFailure::Reparse)),
        Err(NodeOpen::Missing | NodeOpen::Io) => Err(WriteFailure::Commit),
    }
}

fn commit_new_file(
    parent: &OwnedFd,
    final_name: &OsStr,
    bytes: &[u8],
    replace: bool,
) -> Result<(), WriteFailure> {
    let (temp_name, mut temp_file) = create_temp(parent)?;
    let finished = finish_temp(
        parent,
        &temp_name,
        &mut temp_file,
        final_name,
        bytes,
        replace,
    );
    if finished.is_err() || !replace {
        cleanup_exact_temp(parent, &temp_name, &temp_file);
    }
    finished?;
    let _ = fsync(parent);
    Ok(())
}

fn finish_temp(
    parent: &OwnedFd,
    temp_name: &OsStr,
    temp_file: &mut File,
    final_name: &OsStr,
    bytes: &[u8],
    replace: bool,
) -> Result<(), WriteFailure> {
    write_durable(temp_file, bytes)?;
    if !name_matches_handle(parent, temp_name, temp_file.as_fd()) {
        return Err(WriteFailure::Commit);
    }
    link_or_rename(parent, temp_name, final_name, replace)
}

fn write_durable(temp_file: &mut File, bytes: &[u8]) -> Result<(), WriteFailure> {
    if temp_file.write_all(bytes).is_err() || temp_file.sync_all().is_err() {
        Err(WriteFailure::Write)
    } else {
        Ok(())
    }
}

fn link_or_rename(
    parent: &OwnedFd,
    temp_name: &OsStr,
    final_name: &OsStr,
    replace: bool,
) -> Result<(), WriteFailure> {
    let commit = if replace {
        renameat(parent, temp_name, parent, final_name)
    } else {
        linkat(parent, temp_name, parent, final_name, AtFlags::empty())
    };
    commit.map_err(|_| WriteFailure::Commit)
}

fn create_temp(parent: &OwnedFd) -> Result<(OsString, File), WriteFailure> {
    for _ in 0..128 {
        if let Some(created) = try_create_temp(parent)? {
            return Ok(created);
        }
    }
    Err(WriteFailure::TempCreate)
}

fn try_create_temp(parent: &OwnedFd) -> Result<Option<(OsString, File)>, WriteFailure> {
    let name = temporary_component().map_err(|_| WriteFailure::TempCreate)?;
    match openat(parent, &name, TEMP_FLAGS, TEMP_MODE) {
        Ok(handle) => Ok(Some((name, File::from(handle)))),
        Err(Errno::EXIST) => Ok(None),
        Err(_) => Err(WriteFailure::TempCreate),
    }
}

fn cleanup_exact_temp(parent: &OwnedFd, name: &OsStr, file: &File) {
    if name_matches_handle(parent, name, file.as_fd()) {
        let _ = unlinkat(parent, name, AtFlags::empty());
    }
}

fn duplicate_directory(handle: &OwnedFd) -> Result<Option<FsDirectory>, PathFailure> {
    let handle = rio::dup(handle).map_err(|_| PathFailure::Io)?;
    Ok(Some(FsDirectory { handle }))
}

fn open_final_directory(
    parent: &OwnedFd,
    name: &OsStr,
) -> Result<Option<FsDirectory>, PathFailure> {
    match open_directory_at(parent, name) {
        Ok(handle) => Ok(Some(FsDirectory { handle })),
        Err(DirectoryOpen::Missing) => Ok(None),
        Err(error) => Err(map_directory(error)),
    }
}

fn walk_parent(
    base: &OwnedFd,
    components: &[OsString],
    create: bool,
) -> Result<Option<(OwnedFd, OsString)>, PathFailure> {
    let final_name = last_component(components)?;
    let parents = &components[..components.len() - 1];
    let current = rio::dup(base).map_err(|_| PathFailure::Io)?;
    let Some(current) = descend(current, parents, create)? else {
        return Ok(None);
    };
    Ok(Some((current, final_name)))
}

fn last_component(components: &[OsString]) -> Result<OsString, PathFailure> {
    match components.last() {
        Some(name) => Ok(name.clone()),
        None => Err(PathFailure::Internal),
    }
}

fn descend(
    mut current: OwnedFd,
    parents: &[OsString],
    create: bool,
) -> Result<Option<OwnedFd>, PathFailure> {
    for component in parents {
        match step_directory(&current, component, create)? {
            Some(directory) => current = directory,
            None => return Ok(None),
        }
    }
    Ok(Some(current))
}

fn step_directory(
    current: &OwnedFd,
    component: &OsStr,
    create: bool,
) -> Result<Option<OwnedFd>, PathFailure> {
    match open_directory_at(current, component) {
        Ok(directory) => Ok(Some(directory)),
        Err(DirectoryOpen::Missing) if create => Ok(Some(create_and_open(current, component)?)),
        Err(DirectoryOpen::Missing) => Ok(None),
        Err(error) => Err(map_directory(error)),
    }
}

fn create_and_open(parent: &OwnedFd, name: &OsStr) -> Result<OwnedFd, PathFailure> {
    if let Err(error) = mkdirat(parent, name, DIRECTORY_MODE) {
        if error != Errno::EXIST {
            return Err(PathFailure::Io);
        }
    }
    open_directory_at(parent, name).map_err(map_directory)
}

fn map_directory(error: DirectoryOpen) -> PathFailure {
    match error {
        DirectoryOpen::Reparse => PathFailure::Reparse,
        DirectoryOpen::NotDirectory => PathFailure::NotDirectory,
        DirectoryOpen::Missing | DirectoryOpen::Io => PathFailure::Io,
    }
}

fn open_directory_at(parent: &OwnedFd, name: &OsStr) -> Result<OwnedFd, DirectoryOpen> {
    match openat(parent, name, DIRECTORY_FLAGS, Mode::empty()) {
        Ok(handle) => Ok(handle),
        Err(error) => Err(classify_directory_error(parent, name, error)),
    }
}

fn classify_directory_error(parent: &OwnedFd, name: &OsStr, error: Errno) -> DirectoryOpen {
    match error {
        Errno::NOENT => DirectoryOpen::Missing,
        Errno::LOOP => DirectoryOpen::Reparse,
        Errno::NOTDIR => classify_not_directory(parent, name),
        _ => DirectoryOpen::Io,
    }
}

fn classify_not_directory(parent: &OwnedFd, name: &OsStr) -> DirectoryOpen {
    match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(metadata) if file_type(&metadata).is_symlink() => DirectoryOpen::Reparse,
        Ok(_) => DirectoryOpen::NotDirectory,
        Err(Errno::NOENT) => DirectoryOpen::Missing,
        Err(_) => DirectoryOpen::Io,
    }
}

fn open_node_at(parent: &OwnedFd, name: &OsStr) -> Result<(OwnedFd, Stat), NodeOpen> {
    let handle = match openat(parent, name, FILE_FLAGS, Mode::empty()) {
        Ok(handle) => handle,
        Err(error) => return Err(classify_node(error)),
    };
    match fstat(&handle) {
        Ok(metadata) => Ok((handle, metadata)),
        Err(_) => Err(NodeOpen::Io),
    }
}

fn classify_node(error: Errno) -> NodeOpen {
    match error {
        Errno::NOENT => NodeOpen::Missing,
        Errno::LOOP => NodeOpen::Reparse,
        _ => NodeOpen::Io,
    }
}

fn read_open_node(parent: &OwnedFd, name: &OsStr) -> Result<ReadNode, PathFailure> {
    let (handle, metadata) = match open_node_at(parent, name) {
        Ok(value) => value,
        Err(error) => return map_read_open(error),
    };
    if !file_type(&metadata).is_file() {
        return Ok(ReadNode::Other);
    }
    read_handle(handle)
}

fn map_read_open(error: NodeOpen) -> Result<ReadNode, PathFailure> {
    match error {
        NodeOpen::Missing => Ok(ReadNode::Absent),
        NodeOpen::Reparse => Err(PathFailure::Reparse),
        NodeOpen::Io => Err(PathFailure::Io),
    }
}

fn read_handle(handle: OwnedFd) -> Result<ReadNode, PathFailure> {
    let mut file = File::from(handle);
    let mut bytes = Vec::new();
    match file.read_to_end(&mut bytes) {
        Ok(_) => Ok(ReadNode::File(bytes)),
        Err(_) => Err(PathFailure::Io),
    }
}

#[cfg(target_os = "linux")]
fn read_entry_openat2(root: &OwnedFd, relative: &Path) -> Option<Result<ReadNode, PathFailure>> {
    read_with_attempts(root, relative, 0)
}

#[cfg(target_os = "linux")]
fn read_with_attempts(
    root: &OwnedFd,
    relative: &Path,
    attempts: u8,
) -> Option<Result<ReadNode, PathFailure>> {
    use rustix::fs::{ResolveFlags, openat2};

    let resolve = ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS | ResolveFlags::NO_MAGICLINKS;
    match openat2(root, relative, FILE_FLAGS, Mode::empty(), resolve) {
        Ok(handle) => Some(read_beneath_handle(handle)),
        Err(Errno::AGAIN) => retry_openat2(root, relative, attempts),
        Err(Errno::NOSYS | Errno::INVAL) => None,
        Err(error) => Some(map_openat2(error)),
    }
}

#[cfg(target_os = "linux")]
fn retry_openat2(
    root: &OwnedFd,
    relative: &Path,
    attempts: u8,
) -> Option<Result<ReadNode, PathFailure>> {
    if attempts < 4 {
        read_with_attempts(root, relative, attempts.saturating_add(1))
    } else {
        Some(Err(PathFailure::Io))
    }
}

#[cfg(target_os = "linux")]
fn map_openat2(error: Errno) -> Result<ReadNode, PathFailure> {
    match error {
        Errno::NOENT => Ok(ReadNode::Absent),
        Errno::LOOP | Errno::XDEV => Err(PathFailure::Reparse),
        _ => Err(PathFailure::Io),
    }
}

#[cfg(target_os = "linux")]
fn read_beneath_handle(handle: OwnedFd) -> Result<ReadNode, PathFailure> {
    let metadata = match fstat(&handle) {
        Ok(metadata) => metadata,
        Err(_) => return Err(PathFailure::Io),
    };
    if file_type(&metadata).is_file() {
        read_handle(handle)
    } else {
        Ok(ReadNode::Other)
    }
}

fn file_type(metadata: &Stat) -> FileType {
    FileType::from_raw_mode(metadata.st_mode)
}

fn entry_name(entry: &rustix::fs::DirEntry) -> &OsStr {
    OsStr::from_bytes(entry.file_name().to_bytes())
}

fn visit_listed<E, F>(
    name: &OsStr,
    seen: usize,
    max_entries: usize,
    visit: &mut F,
) -> Result<usize, ScanFailure<E>>
where
    F: FnMut(&OsStr) -> Result<(), E>,
{
    if is_dot(name) {
        return Ok(seen);
    }
    if seen == max_entries {
        return Err(ScanFailure::TooMany);
    }
    visit(name).map_err(ScanFailure::Visit)?;
    Ok(seen.saturating_add(1))
}

fn is_dot(name: &OsStr) -> bool {
    name == OsStr::new(".") || name == OsStr::new("..")
}

fn classify_stat(
    parent: &OwnedFd,
    name: &OsStr,
    metadata: &Stat,
) -> Result<Inspected<FsDirectory>, ScanFailure<()>> {
    match stat_kind(file_type(metadata)) {
        StatKind::Link => Ok(Inspected::Link),
        StatKind::File => Ok(Inspected::File),
        StatKind::Other => Ok(Inspected::Other),
        StatKind::Dir => open_inspected_dir(parent, name),
    }
}

enum StatKind {
    Link,
    File,
    Dir,
    Other,
}

fn stat_kind(file_type: FileType) -> StatKind {
    if file_type.is_symlink() {
        return StatKind::Link;
    }
    if file_type.is_file() {
        return StatKind::File;
    }
    if file_type.is_dir() {
        StatKind::Dir
    } else {
        StatKind::Other
    }
}

fn open_inspected_dir(
    parent: &OwnedFd,
    name: &OsStr,
) -> Result<Inspected<FsDirectory>, ScanFailure<()>> {
    match open_directory_at(parent, name) {
        Ok(handle) => Ok(Inspected::Directory(FsDirectory { handle })),
        Err(DirectoryOpen::Reparse) => Ok(Inspected::Link),
        Err(_) => Err(ScanFailure::Io),
    }
}

fn walk_for_remove(
    base: &OwnedFd,
    components: &[OsString],
) -> Result<Option<(OwnedFd, OsString)>, RemoveFailure> {
    match walk_parent(base, components, false) {
        Ok(value) => Ok(value),
        Err(error) => Err(RemoveFailure::Path(error)),
    }
}

fn remove_opened_file<F>(
    parent: &OwnedFd,
    name: &OsStr,
    accept: F,
) -> Result<RemoveStatus, RemoveFailure>
where
    F: FnOnce(&mut File) -> io::Result<bool>,
{
    let Some(file) = open_file_for_remove(parent, name)? else {
        return Ok(RemoveStatus::Absent);
    };
    if !accept_opened(parent, name, file, accept)? {
        return Ok(RemoveStatus::Rejected);
    }
    unlink_named(parent, name, AtFlags::empty())
}

fn open_file_for_remove(parent: &OwnedFd, name: &OsStr) -> Result<Option<File>, RemoveFailure> {
    let Some((handle, metadata)) = open_remove_node(parent, name)? else {
        return Ok(None);
    };
    if file_type(&metadata).is_file() && name_matches_handle(parent, name, &handle) {
        Ok(Some(File::from(handle)))
    } else {
        Err(RemoveFailure::ManagedTree)
    }
}

fn open_remove_node(
    parent: &OwnedFd,
    name: &OsStr,
) -> Result<Option<(OwnedFd, Stat)>, RemoveFailure> {
    match open_node_at(parent, name) {
        Ok(value) => Ok(Some(value)),
        Err(error) => map_remove_open(error),
    }
}

fn map_remove_open(error: NodeOpen) -> Result<Option<(OwnedFd, Stat)>, RemoveFailure> {
    match error {
        NodeOpen::Missing => Ok(None),
        NodeOpen::Reparse => Err(RemoveFailure::ManagedLink),
        NodeOpen::Io => Err(RemoveFailure::ManagedRemove),
    }
}

fn accept_opened<F>(
    parent: &OwnedFd,
    name: &OsStr,
    mut file: File,
    accept: F,
) -> Result<bool, RemoveFailure>
where
    F: FnOnce(&mut File) -> io::Result<bool>,
{
    let accepted = accept(&mut file).map_err(|_| RemoveFailure::ManagedRemove)?;
    if !accepted {
        return Ok(false);
    }
    if name_matches_handle(parent, name, &file) {
        Ok(true)
    } else {
        Err(RemoveFailure::ManagedTree)
    }
}

fn unlink_named(
    parent: &OwnedFd,
    name: &OsStr,
    flags: AtFlags,
) -> Result<RemoveStatus, RemoveFailure> {
    unlinkat(parent, name, flags).map_err(|_| RemoveFailure::ManagedRemove)?;
    Ok(RemoveStatus::Removed)
}

fn open_dir_for_remove(parent: &OwnedFd, name: &OsStr) -> Result<Option<OwnedFd>, RemoveFailure> {
    let Some((handle, metadata)) = open_remove_node(parent, name)? else {
        return Ok(None);
    };
    if file_type(&metadata).is_dir() {
        Ok(Some(handle))
    } else {
        Err(RemoveFailure::ManagedTree)
    }
}

fn remove_if_empty(
    parent: &OwnedFd,
    name: &OsStr,
    directory: OwnedFd,
) -> Result<RemoveStatus, RemoveFailure> {
    if !directory_has_only_dots(&directory)? {
        return Err(RemoveFailure::ManagedTree);
    }
    if !name_matches_handle(parent, name, &directory) {
        return Err(RemoveFailure::ManagedTree);
    }
    unlink_named(parent, name, AtFlags::REMOVEDIR)
}

fn directory_has_only_dots(directory: &OwnedFd) -> Result<bool, RemoveFailure> {
    let mut entries = Dir::read_from(directory).map_err(|_| RemoveFailure::ManagedRemove)?;
    while let Some(entry) = entries.read() {
        let entry = entry.map_err(|_| RemoveFailure::ManagedRemove)?;
        if !is_dot(entry_name(&entry)) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn name_matches_handle(parent: &OwnedFd, name: &OsStr, handle: impl AsFd) -> bool {
    let Ok(opened) = fstat(&handle) else {
        return false;
    };
    let Ok(named) = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) else {
        return false;
    };
    opened.st_dev == named.st_dev && opened.st_ino == named.st_ino
}
