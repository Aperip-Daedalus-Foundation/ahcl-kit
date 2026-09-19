// crates/ahcl-kit-cargo/src/platform_fs/unix.rs - Unix package evidence capability.
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

pub(crate) struct PackageDirectory {
    handle: OwnedFd,
}

enum DirectoryOpenError {
    Missing,
    Reparse,
    NotDirectory,
    Io(Errno),
}

impl PackageDirectory {
    pub(crate) fn open(path: &Path) -> Result<Self, PackageFsError> {
        if !path.is_absolute() {
            return Err(PackageFsError::InvalidPath);
        }

        let mut components = Vec::new();
        let mut saw_root = false;
        for component in path.components() {
            match component {
                Component::RootDir if !saw_root => saw_root = true,
                Component::Normal(value) if saw_root => components.push(value.to_os_string()),
                _ => return Err(PackageFsError::InvalidPath),
            }
        }
        if !saw_root {
            return Err(PackageFsError::InvalidPath);
        }

        let mut current = open(Path::new("/"), DIRECTORY_FLAGS, Mode::empty())
            .map_err(|error| PackageFsError::Io(io_error(error)))?;
        for component in components {
            current = open_directory_at(&current, &component).map_err(map_directory_error)?;
        }
        Ok(Self { handle: current })
    }

    pub(crate) fn root_license_candidates(
        &self,
        max_files: u64,
    ) -> Result<Vec<PathBuf>, PackageFsError> {
        let mut candidates = Vec::new();
        let mut portable = BTreeSet::new();
        let mut entries =
            Dir::read_from(&self.handle).map_err(|error| PackageFsError::Io(io_error(error)))?;
        while let Some(entry) = entries.read() {
            let entry = entry.map_err(|error| PackageFsError::Io(io_error(error)))?;
            let name = OsStr::from_bytes(entry.file_name().to_bytes());
            if name == OsStr::new(".") || name == OsStr::new("..") {
                continue;
            }
            let name = name.to_str().ok_or(PackageFsError::PathEncoding)?;
            let upper = name.to_ascii_uppercase();
            if !["LICENSE", "COPYING", "NOTICE", "COPYRIGHT"]
                .iter()
                .any(|prefix| upper.starts_with(prefix))
            {
                continue;
            }
            if open_regular_file_at(&self.handle, OsStr::new(name))?.is_none() {
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
        let mut current =
            rio::dup(&self.handle).map_err(|error| PackageFsError::Io(io_error(error)))?;
        for component in parents {
            current = open_directory_at(&current, component).map_err(map_directory_error)?;
        }
        let Some((handle, advertised_len)) = open_regular_file_at(&current, final_name)? else {
            return Ok(None);
        };
        read_file_with_limit(File::from(handle), advertised_len, limit).map(Some)
    }
}

pub(crate) fn read_regular_file(path: &Path) -> Result<Vec<u8>, PackageFsError> {
    let parent = path.parent().ok_or(PackageFsError::InvalidPath)?;
    let name = path.file_name().ok_or(PackageFsError::InvalidPath)?;
    let directory = PackageDirectory::open(parent)?;
    let Some((handle, advertised_len)) = open_regular_file_at(&directory.handle, name)? else {
        return Err(PackageFsError::InvalidPath);
    };
    read_file_with_limit(File::from(handle), advertised_len, u64::MAX)
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
        Errno::NOTDIR => match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(metadata) if FileType::from_raw_mode(metadata.st_mode).is_symlink() => {
                DirectoryOpenError::Reparse
            }
            Ok(_) => DirectoryOpenError::NotDirectory,
            Err(Errno::NOENT) => DirectoryOpenError::Missing,
            Err(source) => DirectoryOpenError::Io(source),
        },
        source => DirectoryOpenError::Io(source),
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

fn sort_candidates(candidates: &mut [PathBuf]) {
    candidates.sort_by(|left, right| {
        left.to_string_lossy()
            .to_ascii_lowercase()
            .cmp(&right.to_string_lossy().to_ascii_lowercase())
            .then_with(|| left.cmp(right))
    });
}
