// crates/ahcl-kit-fs/src/windows/walk.rs - Windows no-follow walks and commits.
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

use super::handle::{
    DIRECTORY_READ_ACCESS, DirOpen, DirectoryHandle, NodeOpen, OpenedNode, create_directory,
    create_temp_file, delete_open_handle, directory_is_empty, flush_directory, open_directory_path,
    open_node_path, open_node_path_with_share, rename_open_file,
};
use super::path::append_component;
use crate::failure::{PathFailure, ReadNode, RemoveFailure, RemoveStatus, WriteFailure};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::GENERIC_READ;
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
    SYNCHRONIZE,
};

pub(super) fn current_directory(chain: &[DirectoryHandle]) -> Option<&DirectoryHandle> {
    chain.last()
}

pub(super) fn select_parent<'a>(
    base: &'a DirectoryHandle,
    directories: &'a [DirectoryHandle],
) -> Option<&'a DirectoryHandle> {
    directories.last().or(Some(base))
}

pub(super) fn walk_parents(
    base: &DirectoryHandle,
    components: &[OsString],
    create: bool,
    access: u32,
) -> Result<Option<(Vec<DirectoryHandle>, OsString)>, PathFailure> {
    let final_name = last_component(components)?;
    let mut directories = Vec::new();
    for component in &components[..components.len() - 1] {
        let Some(opened) = open_next(&directories, base, component, create, access)? else {
            return Ok(None);
        };
        directories.push(opened);
    }
    Ok(Some((directories, final_name)))
}

fn open_next(
    directories: &[DirectoryHandle],
    base: &DirectoryHandle,
    component: &OsStr,
    create: bool,
    access: u32,
) -> Result<Option<DirectoryHandle>, PathFailure> {
    let parent = select_parent(base, directories).ok_or(PathFailure::Internal)?;
    let child_path = append_component(&parent.final_path, component);
    open_or_create(&child_path, create, access)
}

fn last_component(components: &[OsString]) -> Result<OsString, PathFailure> {
    match components.last() {
        Some(name) => Ok(name.clone()),
        None => Err(PathFailure::Internal),
    }
}

fn open_or_create(
    child_path: &Path,
    create: bool,
    access: u32,
) -> Result<Option<DirectoryHandle>, PathFailure> {
    match open_directory_path(child_path, access) {
        Ok(directory) => Ok(Some(directory)),
        Err(DirOpen::Missing) if create => reopen_created(child_path, access),
        Err(DirOpen::Missing) => Ok(None),
        Err(error) => Err(map_dir(error)),
    }
}

fn reopen_created(path: &Path, access: u32) -> Result<Option<DirectoryHandle>, PathFailure> {
    create_directory(path).map_err(|_| PathFailure::Io)?;
    open_directory_path(path, access).map(Some).map_err(map_dir)
}

pub(super) fn map_dir(error: DirOpen) -> PathFailure {
    match error {
        DirOpen::Reparse => PathFailure::Reparse,
        DirOpen::NotDirectory => PathFailure::NotDirectory,
        DirOpen::Missing | DirOpen::Io(_) => PathFailure::Io,
    }
}

pub(super) fn read_named(parent: &DirectoryHandle, name: &OsStr) -> Result<ReadNode, PathFailure> {
    let node_path = append_component(&parent.final_path, name);
    let Some(node) = open_entry_node(&node_path)? else {
        return Ok(ReadNode::Absent);
    };
    read_opened_entry(node)
}

fn open_entry_node(path: &Path) -> Result<Option<OpenedNode>, PathFailure> {
    match open_node_path(path, GENERIC_READ | FILE_READ_ATTRIBUTES | SYNCHRONIZE) {
        Ok(node) => Ok(Some(node)),
        Err(NodeOpen::Missing) => Ok(None),
        Err(NodeOpen::Reparse) => Err(PathFailure::Reparse),
        Err(NodeOpen::Io) => Err(PathFailure::Io),
    }
}

fn read_opened_entry(node: OpenedNode) -> Result<ReadNode, PathFailure> {
    if node.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        return Ok(ReadNode::Other);
    }
    let mut bytes = Vec::new();
    let mut file = node.file;
    match file.read_to_end(&mut bytes) {
        Ok(_) => Ok(ReadNode::File(bytes)),
        Err(_) => Err(PathFailure::Io),
    }
}

pub(super) fn ensure_replace_target(
    parent: &DirectoryHandle,
    name: &OsStr,
) -> Result<(), WriteFailure> {
    let target_path = append_component(&parent.final_path, name);
    match open_node_path(&target_path, FILE_READ_ATTRIBUTES | SYNCHRONIZE) {
        Ok(node) if node.attributes & FILE_ATTRIBUTE_DIRECTORY == 0 => Ok(()),
        Ok(_) => Err(WriteFailure::Commit),
        Err(NodeOpen::Reparse) => Err(WriteFailure::Path(PathFailure::Reparse)),
        Err(NodeOpen::Missing | NodeOpen::Io) => Err(WriteFailure::Commit),
    }
}

pub(super) fn commit_temp_file(
    parent: &DirectoryHandle,
    final_name: &OsStr,
    bytes: &[u8],
    replace: bool,
) -> Result<(), WriteFailure> {
    let (_temp_name, mut temp_file) =
        create_temp_file(parent).map_err(|_| WriteFailure::TempCreate)?;
    if !write_temp_file(&mut temp_file, bytes) {
        let _ = delete_open_handle(temp_file.as_raw_handle());
        return Err(WriteFailure::Write);
    }
    let target_path = append_component(&parent.final_path, final_name);
    if rename_open_file(temp_file.as_raw_handle(), target_path.as_os_str(), replace).is_err() {
        let _ = delete_open_handle(temp_file.as_raw_handle());
        return Err(WriteFailure::Commit);
    }
    flush_directory(&parent.file);
    Ok(())
}

fn write_temp_file(file: &mut File, bytes: &[u8]) -> bool {
    file.write_all(bytes).is_ok() && file.sync_all().is_ok()
}

pub(super) fn walk_managed(
    chain: &[DirectoryHandle],
    components: &[OsString],
) -> Result<Option<PathBuf>, RemoveFailure> {
    let base = current_directory(chain).ok_or(RemoveFailure::Path(PathFailure::Internal))?;
    let final_name = managed_final_name(components)?;
    let directories = walk_managed_parents(base, components)?;
    let Some(directories) = directories else {
        return Ok(None);
    };
    managed_target(base, &directories, &final_name)
}

fn managed_final_name(components: &[OsString]) -> Result<OsString, RemoveFailure> {
    match components.last() {
        Some(name) => Ok(name.clone()),
        None => Err(RemoveFailure::Path(PathFailure::Internal)),
    }
}

fn managed_target(
    base: &DirectoryHandle,
    directories: &[DirectoryHandle],
    final_name: &OsStr,
) -> Result<Option<PathBuf>, RemoveFailure> {
    let parent =
        select_parent(base, directories).ok_or(RemoveFailure::Path(PathFailure::Internal))?;
    Ok(Some(append_component(&parent.final_path, final_name)))
}

fn walk_managed_parents(
    base: &DirectoryHandle,
    components: &[OsString],
) -> Result<Option<Vec<DirectoryHandle>>, RemoveFailure> {
    let mut directories = Vec::new();
    for component in &components[..components.len() - 1] {
        let parent =
            select_parent(base, &directories).ok_or(RemoveFailure::Path(PathFailure::Internal))?;
        let child_path = append_component(&parent.final_path, component);
        let Some(directory) = open_managed_child(&child_path)? else {
            return Ok(None);
        };
        directories.push(directory);
    }
    Ok(Some(directories))
}

fn open_managed_child(path: &Path) -> Result<Option<DirectoryHandle>, RemoveFailure> {
    match open_directory_path(path, DIRECTORY_READ_ACCESS) {
        Ok(directory) => Ok(Some(directory)),
        Err(DirOpen::Missing) => Ok(None),
        Err(error) => Err(map_managed_dir(error)),
    }
}

fn map_managed_dir(error: DirOpen) -> RemoveFailure {
    match error {
        DirOpen::Reparse => RemoveFailure::ManagedLink,
        DirOpen::NotDirectory => RemoveFailure::ManagedTree,
        DirOpen::Missing | DirOpen::Io(_) => RemoveFailure::ManagedRemove,
    }
}

pub(super) fn remove_matching_file<F>(path: &Path, accept: F) -> Result<RemoveStatus, RemoveFailure>
where
    F: FnOnce(&mut File) -> io::Result<bool>,
{
    let Some(node) = open_managed_file(path)? else {
        return Ok(RemoveStatus::Absent);
    };
    if node.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        return Err(RemoveFailure::ManagedTree);
    }
    delete_if_accepted(node.file, accept)
}

fn open_managed_file(path: &Path) -> Result<Option<OpenedNode>, RemoveFailure> {
    match open_node_path_with_share(
        path,
        DELETE | GENERIC_READ | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_SHARE_READ,
    ) {
        Ok(node) => Ok(Some(node)),
        Err(NodeOpen::Missing) => Ok(None),
        Err(NodeOpen::Reparse) => Err(RemoveFailure::ManagedLink),
        Err(NodeOpen::Io) => Err(RemoveFailure::ManagedRemove),
    }
}

fn delete_if_accepted<F>(mut file: File, accept: F) -> Result<RemoveStatus, RemoveFailure>
where
    F: FnOnce(&mut File) -> io::Result<bool>,
{
    let accepted = accept(&mut file).map_err(|_| RemoveFailure::ManagedRemove)?;
    if !accepted {
        return Ok(RemoveStatus::Rejected);
    }
    delete_open_handle(file.as_raw_handle()).map_err(|_| RemoveFailure::ManagedRemove)?;
    Ok(RemoveStatus::Removed)
}

pub(super) fn remove_empty_directory(path: &Path) -> Result<RemoveStatus, RemoveFailure> {
    let Some(node) = open_managed_directory(path)? else {
        return Ok(RemoveStatus::Absent);
    };
    delete_directory_if_empty(node)
}

fn delete_directory_if_empty(node: OpenedNode) -> Result<RemoveStatus, RemoveFailure> {
    if node.attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(RemoveFailure::ManagedTree);
    }
    let directory_path = node.final_path.ok_or(RemoveFailure::ManagedTree)?;
    if !directory_is_empty(&directory_path).map_err(|_| RemoveFailure::ManagedRemove)? {
        return Err(RemoveFailure::ManagedTree);
    }
    delete_open_handle(node.file.as_raw_handle()).map_err(|_| RemoveFailure::ManagedRemove)?;
    Ok(RemoveStatus::Removed)
}

fn open_managed_directory(path: &Path) -> Result<Option<OpenedNode>, RemoveFailure> {
    match open_node_path(
        path,
        DELETE | FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
    ) {
        Ok(node) => Ok(Some(node)),
        Err(NodeOpen::Missing) => Ok(None),
        Err(NodeOpen::Reparse) => Err(RemoveFailure::ManagedLink),
        Err(NodeOpen::Io) => Err(RemoveFailure::ManagedRemove),
    }
}
