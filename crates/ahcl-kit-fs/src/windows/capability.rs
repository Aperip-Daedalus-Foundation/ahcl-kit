// crates/ahcl-kit-fs/src/windows/capability.rs - Windows no-follow project tree reads and writes.
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

use super::handle::{
    DIRECTORY_READ_ACCESS, DirOpen, DirectoryHandle, OPEN_NO_REPARSE, open_directory_path,
    query_attributes,
};
use super::path::{FormError, append_component, split_absolute};
use super::walk::{
    commit_temp_file, current_directory, ensure_replace_target, map_dir, read_named,
    remove_empty_directory, remove_matching_file, select_parent, walk_managed, walk_parents,
};
use crate::failure::{
    Inspected, PathFailure, ReadNode, RemoveFailure, RemoveStatus, RootFailure, ScanFailure,
    WriteFailure,
};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::path::Path;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ADD_FILE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    SYNCHRONIZE,
};

const DIRECTORY_WRITE_ACCESS: u32 = DIRECTORY_READ_ACCESS | FILE_ADD_FILE;

pub struct FsRoot {
    root_chain: Vec<DirectoryHandle>,
}

pub struct FsDirectory {
    chain: Vec<DirectoryHandle>,
}

struct WriteParent {
    operation_root: DirectoryHandle,
    directories: Vec<DirectoryHandle>,
    final_name: OsString,
}

impl FsRoot {
    pub fn open(path: &Path) -> Result<Self, RootFailure> {
        let (volume_root, components) = split_project(path)?;
        if components.is_empty() {
            return Err(RootFailure::FilesystemRoot);
        }
        let mut root_chain = vec![open_root_directory(&volume_root)?];
        for component in components {
            root_chain.push(open_root_child(&root_chain, &component)?);
        }
        Ok(Self { root_chain })
    }

    pub fn read_entry(
        &self,
        relative: &Path,
        components: &[OsString],
    ) -> Result<ReadNode, PathFailure> {
        let _ = relative;
        let base = project_directory(&self.root_chain)?;
        let Some((directories, final_name)) =
            walk_parents(base, components, false, DIRECTORY_READ_ACCESS)?
        else {
            return Ok(ReadNode::Absent);
        };
        let parent = select_parent(base, &directories).ok_or(PathFailure::Internal)?;
        read_named(parent, &final_name)
    }

    pub fn write_file(
        &self,
        components: &[OsString],
        bytes: &[u8],
        replace: bool,
    ) -> Result<(), WriteFailure> {
        let prepared = self.prepare_write(components)?;
        let parent = select_parent(&prepared.operation_root, &prepared.directories)
            .ok_or(WriteFailure::Path(PathFailure::Internal))?;
        if replace {
            ensure_replace_target(parent, &prepared.final_name)?;
        }
        commit_temp_file(parent, &prepared.final_name, bytes, replace)
    }

    pub fn open_directory(
        &self,
        components: &[OsString],
    ) -> Result<Option<FsDirectory>, PathFailure> {
        if components.is_empty() {
            return Ok(Some(FsDirectory { chain: Vec::new() }));
        }
        let base = project_directory(&self.root_chain)?;
        let Some((directories, final_name)) =
            walk_parents(base, components, false, DIRECTORY_READ_ACCESS)?
        else {
            return Ok(None);
        };
        push_final_directory(base, directories, &final_name)
    }

    fn prepare_write(&self, components: &[OsString]) -> Result<WriteParent, WriteFailure> {
        let operation_root = reopen_for_write(&self.root_chain)?;
        let Some((directories, final_name)) =
            walk_parents(&operation_root, components, true, DIRECTORY_WRITE_ACCESS)
                .map_err(WriteFailure::Path)?
        else {
            return Err(WriteFailure::Path(PathFailure::Io));
        };
        Ok(WriteParent {
            operation_root,
            directories,
            final_name,
        })
    }
}

impl FsDirectory {
    pub fn scan_names<E, F>(&self, max_entries: usize, mut visit: F) -> Result<(), ScanFailure<E>>
    where
        F: FnMut(&OsStr) -> Result<(), E>,
    {
        let directory = current_directory(&self.chain).ok_or(ScanFailure::Io)?;
        let mut seen = 0_usize;
        for entry in std::fs::read_dir(&directory.final_path).map_err(|_| ScanFailure::Io)? {
            if seen == max_entries {
                return Err(ScanFailure::TooMany);
            }
            let name = entry.map_err(|_| ScanFailure::Io)?.file_name();
            visit(&name).map_err(ScanFailure::Visit)?;
            seen = seen.saturating_add(1);
        }
        Ok(())
    }

    pub fn inspect(&self, name: &OsStr) -> Result<Inspected<FsDirectory>, ScanFailure<()>> {
        let parent = current_directory(&self.chain).ok_or(ScanFailure::Io)?;
        inspect_child(parent, name)
    }

    pub fn remove_file<F>(
        &self,
        components: &[OsString],
        accept: F,
    ) -> Result<RemoveStatus, RemoveFailure>
    where
        F: FnOnce(&mut File) -> io::Result<bool>,
    {
        let Some(path) = walk_managed(&self.chain, components)? else {
            return Ok(RemoveStatus::Absent);
        };
        remove_matching_file(&path, accept)
    }

    pub fn remove_empty_directory(
        &self,
        components: &[OsString],
    ) -> Result<RemoveStatus, RemoveFailure> {
        let Some(path) = walk_managed(&self.chain, components)? else {
            return Ok(RemoveStatus::Absent);
        };
        remove_empty_directory(&path)
    }
}

fn split_project(path: &Path) -> Result<(std::path::PathBuf, Vec<OsString>), RootFailure> {
    match split_absolute(path, true) {
        Ok(parsed) => Ok(parsed),
        Err(FormError::NotAbsolute) => Err(RootFailure::NotAbsolute),
        Err(FormError::Invalid) => Err(RootFailure::Invalid),
    }
}

fn open_root_directory(path: &Path) -> Result<DirectoryHandle, RootFailure> {
    open_directory_path(path, DIRECTORY_READ_ACCESS).map_err(map_root_dir)
}

fn open_root_child(
    chain: &[DirectoryHandle],
    component: &OsStr,
) -> Result<DirectoryHandle, RootFailure> {
    let parent = current_directory(chain)
        .map(|directory| directory.final_path.as_path())
        .ok_or(RootFailure::Open)?;
    let child = append_component(parent, component);
    open_root_directory(&child)
}

fn map_root_dir(error: DirOpen) -> RootFailure {
    match error {
        DirOpen::Reparse => RootFailure::Reparse,
        DirOpen::NotDirectory => RootFailure::NotDirectory,
        DirOpen::Missing | DirOpen::Io(_) => RootFailure::Open,
    }
}

fn project_directory(chain: &[DirectoryHandle]) -> Result<&DirectoryHandle, PathFailure> {
    current_directory(chain).ok_or(PathFailure::Internal)
}

fn reopen_for_write(chain: &[DirectoryHandle]) -> Result<DirectoryHandle, WriteFailure> {
    let current = current_directory(chain).ok_or(WriteFailure::Path(PathFailure::Internal))?;
    open_directory_path(&current.final_path, DIRECTORY_WRITE_ACCESS)
        .map_err(|error| WriteFailure::Path(map_dir(error)))
}

fn push_final_directory(
    base: &DirectoryHandle,
    mut directories: Vec<DirectoryHandle>,
    final_name: &OsStr,
) -> Result<Option<FsDirectory>, PathFailure> {
    let parent = select_parent(base, &directories).ok_or(PathFailure::Internal)?;
    let path = append_component(&parent.final_path, final_name);
    match open_directory_path(&path, DIRECTORY_READ_ACCESS) {
        Ok(directory) => {
            directories.push(directory);
            Ok(Some(FsDirectory { chain: directories }))
        }
        Err(DirOpen::Missing) => Ok(None),
        Err(error) => Err(map_dir(error)),
    }
}

fn inspect_child(
    parent: &DirectoryHandle,
    name: &OsStr,
) -> Result<Inspected<FsDirectory>, ScanFailure<()>> {
    let path = append_component(&parent.final_path, name);
    let file = super::handle::open_existing(
        &path,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ
            | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
        OPEN_NO_REPARSE,
    )
    .map_err(|_| ScanFailure::Io)?;
    classify_opened(file)
}

fn classify_opened(file: std::fs::File) -> Result<Inspected<FsDirectory>, ScanFailure<()>> {
    let attributes = query_attributes(&file).map_err(|_| ScanFailure::Io)?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Ok(Inspected::Link);
    }
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Ok(Inspected::File);
    }
    let final_path = super::handle::final_path(&file).map_err(|_| ScanFailure::Io)?;
    Ok(Inspected::Directory(FsDirectory {
        chain: vec![DirectoryHandle { file, final_path }],
    }))
}
