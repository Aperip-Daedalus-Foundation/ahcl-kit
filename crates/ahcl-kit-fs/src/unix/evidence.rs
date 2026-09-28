// crates/ahcl-kit-fs/src/unix/evidence.rs - Unix no-follow package evidence reads.
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

use crate::{PackageFsError, read_file_with_limit};
use rustix::fd::OwnedFd;
use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, fstat, open, openat, statat};
use rustix::io::{self as rio, Errno};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const FILE_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

pub struct PackageDirectory {
    handle: OwnedFd,
}

enum DirectoryOpenError {
    Missing,
    Reparse,
    NotDirectory,
    Io(Errno),
}

impl PackageDirectory {
    pub fn open(path: &Path) -> Result<Self, PackageFsError> {
        let components = absolute_components(path)?;
        // Descend from the filesystem root with no-follow directory opens.
        // Reparse points and non-directories remain hard failures.
        let mut current = open_root()?;
        for component in components {
            current = open_directory_at(&current, &component).map_err(map_directory_error)?;
        }
        Ok(Self { handle: current })
    }

    pub fn root_license_candidates(&self, max_files: u64) -> Result<Vec<PathBuf>, PackageFsError> {
        let mut candidates = Vec::new();
        let mut portable = BTreeSet::new();
        let mut entries =
            Dir::read_from(&self.handle).map_err(|error| PackageFsError::Io(io_error(error)))?;
        while let Some(entry) = entries.read() {
            let entry = entry.map_err(|error| PackageFsError::Io(io_error(error)))?;
            self.consider_license_entry(&entry, &mut candidates, &mut portable, max_files)?;
        }
        sort_candidates(&mut candidates);
        Ok(candidates)
    }

    fn consider_license_entry(
        &self,
        entry: &rustix::fs::DirEntry,
        candidates: &mut Vec<PathBuf>,
        portable: &mut BTreeSet<String>,
        max_files: u64,
    ) -> Result<(), PackageFsError> {
        let Some(name) = license_file_name(entry)? else {
            return Ok(());
        };
        // The no-follow regular-file open rejects links before the name is kept.
        if open_regular_file_at(&self.handle, OsStr::new(&name))?.is_none() {
            return Ok(());
        }
        push_unique_candidate(candidates, portable, &name, max_files)
    }

    pub fn read_bounded_file(
        &self,
        relative: &Path,
        limit: u64,
    ) -> Result<Option<Vec<u8>>, PackageFsError> {
        let (parent, name) = self.open_relative(relative)?;
        let Some((handle, advertised_len)) = open_regular_file_at(&parent, &name)? else {
            return Ok(None);
        };
        read_file_with_limit(File::from(handle), advertised_len, limit).map(Some)
    }

    fn open_relative(&self, relative: &Path) -> Result<(OwnedFd, OsString), PackageFsError> {
        let components = normal_components(relative)?;
        let Some((final_name, parents)) = components.split_last() else {
            return Err(PackageFsError::InvalidPath);
        };
        let name = final_name.clone();
        let mut current =
            rio::dup(&self.handle).map_err(|error| PackageFsError::Io(io_error(error)))?;
        for component in parents {
            current = open_directory_at(&current, component).map_err(map_directory_error)?;
        }
        Ok((current, name))
    }
}

pub fn read_regular_file(path: &Path) -> Result<Vec<u8>, PackageFsError> {
    let parent = path.parent().ok_or(PackageFsError::InvalidPath)?;
    let name = path.file_name().ok_or(PackageFsError::InvalidPath)?;
    let directory = PackageDirectory::open(parent)?;
    let Some((handle, advertised_len)) = open_regular_file_at(&directory.handle, name)? else {
        return Err(PackageFsError::InvalidPath);
    };
    read_file_with_limit(File::from(handle), advertised_len, u64::MAX)
}

pub fn validate_regular_file(root: &Path, relative: &Path) -> Result<(), PackageFsError> {
    let directory = PackageDirectory::open(root)?;
    let components = normal_components(relative)?;
    let (final_name, parents) = components.split_last().ok_or(PackageFsError::InvalidPath)?;
    let mut current =
        rio::dup(&directory.handle).map_err(|error| PackageFsError::Io(io_error(error)))?;
    for component in parents {
        current = open_directory_at(&current, component).map_err(map_directory_error)?;
    }
    open_regular_file_at(&current, final_name)?
        .map(|_| ())
        .ok_or(PackageFsError::InvalidPath)
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

fn open_directory_at(parent: &OwnedFd, name: &OsStr) -> Result<OwnedFd, DirectoryOpenError> {
    match openat(parent, name, DIRECTORY_FLAGS, Mode::empty()) {
        Ok(handle) => Ok(handle),
        Err(error) => Err(classify_directory_error(parent, name, error)),
    }
}

fn open_regular_file_at(
    parent: &OwnedFd,
    name: &OsStr,
) -> Result<Option<(OwnedFd, u64)>, PackageFsError> {
    let handle = openat(parent, name, FILE_FLAGS, Mode::empty()).map_err(|error| {
        if error == Errno::LOOP {
            PackageFsError::LinkOrReparsePoint
        } else {
            PackageFsError::Io(io_error(error))
        }
    })?;
    let metadata = fstat(&handle).map_err(|error| PackageFsError::Io(io_error(error)))?;
    if !FileType::from_raw_mode(metadata.st_mode).is_file() {
        return Ok(None);
    }
    let advertised_len = u64::try_from(metadata.st_size)
        .map_err(|_| PackageFsError::Io(io::Error::other("negative file length")))?;
    Ok(Some((handle, advertised_len)))
}

fn classify_directory_error(parent: &OwnedFd, name: &OsStr, error: Errno) -> DirectoryOpenError {
    match error {
        Errno::NOENT => DirectoryOpenError::Missing,
        Errno::LOOP => DirectoryOpenError::Reparse,
        Errno::NOTDIR => classify_not_directory(parent, name),
        source => DirectoryOpenError::Io(source),
    }
}

fn classify_not_directory(parent: &OwnedFd, name: &OsStr) -> DirectoryOpenError {
    // The failed open did not follow a link. This stat confirms whether the
    // entry itself is a symlink or simply not a directory.
    match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(metadata) if FileType::from_raw_mode(metadata.st_mode).is_symlink() => {
            DirectoryOpenError::Reparse
        }
        Ok(_) => DirectoryOpenError::NotDirectory,
        Err(Errno::NOENT) => DirectoryOpenError::Missing,
        Err(source) => DirectoryOpenError::Io(source),
    }
}

fn map_directory_error(error: DirectoryOpenError) -> PackageFsError {
    match error {
        DirectoryOpenError::Reparse => PackageFsError::LinkOrReparsePoint,
        DirectoryOpenError::Missing | DirectoryOpenError::NotDirectory => {
            PackageFsError::InvalidPath
        }
        DirectoryOpenError::Io(source) => PackageFsError::Io(io_error(source)),
    }
}

fn io_error(error: Errno) -> io::Error {
    io::Error::from_raw_os_error(error.raw_os_error())
}

enum ComponentPiece {
    Root,
    Normal(OsString),
}

fn absolute_components(path: &Path) -> Result<Vec<OsString>, PackageFsError> {
    if !path.is_absolute() {
        return Err(PackageFsError::InvalidPath);
    }
    collect_absolute_components(path)
}

fn collect_absolute_components(path: &Path) -> Result<Vec<OsString>, PackageFsError> {
    let mut components = Vec::new();
    let mut saw_root = false;
    for component in path.components() {
        apply_component(&mut components, &mut saw_root, component)?;
    }
    if saw_root {
        Ok(components)
    } else {
        Err(PackageFsError::InvalidPath)
    }
}

fn apply_component(
    components: &mut Vec<OsString>,
    saw_root: &mut bool,
    component: Component,
) -> Result<(), PackageFsError> {
    match component_piece(component, *saw_root)? {
        ComponentPiece::Root => *saw_root = true,
        ComponentPiece::Normal(value) => components.push(value),
    }
    Ok(())
}

fn component_piece(component: Component, saw_root: bool) -> Result<ComponentPiece, PackageFsError> {
    match component {
        Component::RootDir if !saw_root => Ok(ComponentPiece::Root),
        Component::Normal(value) if saw_root => Ok(ComponentPiece::Normal(value.to_os_string())),
        _ => Err(PackageFsError::InvalidPath),
    }
}

fn open_root() -> Result<OwnedFd, PackageFsError> {
    open(Path::new("/"), DIRECTORY_FLAGS, Mode::empty())
        .map_err(|error| PackageFsError::Io(io_error(error)))
}

fn license_file_name(entry: &rustix::fs::DirEntry) -> Result<Option<String>, PackageFsError> {
    let name = OsStr::from_bytes(entry.file_name().to_bytes());
    if name == OsStr::new(".") || name == OsStr::new("..") {
        return Ok(None);
    }
    let name = name.to_str().ok_or(PackageFsError::PathEncoding)?;
    if is_license_name(name) {
        Ok(Some(name.to_owned()))
    } else {
        Ok(None)
    }
}

fn is_license_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["LICENSE", "COPYING", "NOTICE", "COPYRIGHT"]
        .iter()
        .any(|prefix| upper.starts_with(prefix))
}

fn push_unique_candidate(
    candidates: &mut Vec<PathBuf>,
    portable: &mut BTreeSet<String>,
    name: &str,
    max_files: u64,
) -> Result<(), PackageFsError> {
    if !portable.insert(name.to_ascii_lowercase()) {
        return Ok(());
    }
    let count = u64::try_from(candidates.len()).map_or(u64::MAX, |length| length.saturating_add(1));
    if count > max_files {
        return Err(PackageFsError::TooManyFiles(count));
    }
    candidates.push(PathBuf::from(name));
    Ok(())
}

fn sort_candidates(candidates: &mut [PathBuf]) {
    candidates.sort_by(|left, right| {
        left.to_string_lossy()
            .to_ascii_lowercase()
            .cmp(&right.to_string_lossy().to_ascii_lowercase())
            .then_with(|| left.cmp(right))
    });
}
