// crates/ahcl-kit-fs/src/windows/evidence.rs - Windows no-follow package evidence reads.
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
    DIRECTORY_READ_ACCESS, DirOpen, DirectoryHandle, OPEN_NO_REPARSE, final_path,
    open_directory_path, open_existing, query_attributes,
};
use super::path::{FormError, append_component, split_absolute, windows_path_eq};
use crate::{PackageFsError, read_file_with_limit};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::path::{Component, Path, PathBuf};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_READ,
    FILE_SHARE_WRITE, SYNCHRONIZE,
};

const FILE_ACCESS: u32 = 0x8000_0000 | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
const SHARE_READ_WRITE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE;

pub struct PackageDirectory {
    chain: Vec<DirectoryHandle>,
}

struct Located {
    directory: PathBuf,
    name: OsString,
}

enum OpenedEvidence {
    File(File),
    NotFile,
}

impl PackageDirectory {
    pub fn open(path: &Path) -> Result<Self, PackageFsError> {
        let (volume_root, components) = split_evidence(path)?;
        let mut chain = vec![open_evidence_directory(&volume_root)?];
        for component in components {
            let parent = chain_final(&chain)?;
            chain.push(open_equal_child(parent, &component)?);
        }
        Ok(Self { chain })
    }

    pub fn root_license_candidates(&self, max_files: u64) -> Result<Vec<PathBuf>, PackageFsError> {
        let root = chain_final(&self.chain)?;
        let mut found = LicenseNames::default();
        for entry in std::fs::read_dir(root).map_err(PackageFsError::Io)? {
            consider_license(root, entry, &mut found, max_files)?;
        }
        Ok(found.finish())
    }

    pub fn read_bounded_file(
        &self,
        relative: &Path,
        limit: u64,
    ) -> Result<Option<Vec<u8>>, PackageFsError> {
        let located = self.locate(relative)?;
        let Some(file) = open_contained_file(&located.directory, &located.name)? else {
            return Ok(None);
        };
        read_file_with_limit(file.0, file.1, limit).map(Some)
    }

    fn locate(&self, relative: &Path) -> Result<Located, PackageFsError> {
        let components = normal_components(relative)?;
        let (name, parents) = components.split_last().ok_or(PackageFsError::InvalidPath)?;
        let directories = walk_evidence(&self.chain, parents)?;
        let directory = final_of(&self.chain, &directories)?.to_path_buf();
        Ok(Located {
            directory,
            name: name.clone(),
        })
    }
}

pub fn read_regular_file(path: &Path) -> Result<Vec<u8>, PackageFsError> {
    let parent = path.parent().ok_or(PackageFsError::InvalidPath)?;
    let name = path.file_name().ok_or(PackageFsError::InvalidPath)?;
    let directory = PackageDirectory::open(parent)?;
    let base = chain_final(&directory.chain)?;
    let (file, len) = open_contained_file(base, name)?.ok_or(PackageFsError::InvalidPath)?;
    read_file_with_limit(file, len, u64::MAX)
}

pub fn validate_regular_file(root: &Path, relative: &Path) -> Result<(), PackageFsError> {
    let directory = PackageDirectory::open(root)?;
    let located = directory.locate(relative)?;
    open_contained_file(&located.directory, &located.name)?
        .map(|_| ())
        .ok_or(PackageFsError::InvalidPath)
}

fn split_evidence(path: &Path) -> Result<(PathBuf, Vec<OsString>), PackageFsError> {
    match split_absolute(path, false) {
        Ok(parsed) => Ok(parsed),
        Err(FormError::NotAbsolute | FormError::Invalid) => Err(PackageFsError::InvalidPath),
    }
}

fn open_evidence_directory(path: &Path) -> Result<DirectoryHandle, PackageFsError> {
    open_directory_path(path, DIRECTORY_READ_ACCESS).map_err(map_directory_error)
}

fn open_equal_child(
    parent_final: &Path,
    component: &OsStr,
) -> Result<DirectoryHandle, PackageFsError> {
    let expected = append_component(parent_final, component);
    let directory = open_evidence_directory(&expected)?;
    // Evidence reads keep this equality check in addition to the reparse-attribute
    // rejection performed by the shared directory open.
    if windows_path_eq(&directory.final_path, &expected) {
        Ok(directory)
    } else {
        Err(PackageFsError::LinkOrReparsePoint)
    }
}

fn map_directory_error(error: DirOpen) -> PackageFsError {
    match error {
        DirOpen::Reparse => PackageFsError::LinkOrReparsePoint,
        DirOpen::Missing | DirOpen::NotDirectory => PackageFsError::InvalidPath,
        DirOpen::Io(source) => PackageFsError::Io(source),
    }
}

fn chain_final(chain: &[DirectoryHandle]) -> Result<&Path, PackageFsError> {
    chain
        .last()
        .map(|directory| directory.final_path.as_path())
        .ok_or(PackageFsError::InvalidPath)
}

fn walk_evidence(
    base: &[DirectoryHandle],
    parents: &[OsString],
) -> Result<Vec<DirectoryHandle>, PackageFsError> {
    let mut directories = Vec::new();
    for component in parents {
        let parent = final_of(base, &directories)?;
        directories.push(open_equal_child(parent, component)?);
    }
    Ok(directories)
}

fn final_of<'a>(
    base: &'a [DirectoryHandle],
    extra: &'a [DirectoryHandle],
) -> Result<&'a Path, PackageFsError> {
    extra
        .last()
        .or(base.last())
        .map(|directory| directory.final_path.as_path())
        .ok_or(PackageFsError::InvalidPath)
}

fn normal_components(path: &Path) -> Result<Vec<OsString>, PackageFsError> {
    if path.as_os_str().is_empty() {
        return Err(PackageFsError::InvalidPath);
    }
    path.components().map(normal_component).collect()
}

fn normal_component(component: Component<'_>) -> Result<OsString, PackageFsError> {
    match component {
        Component::Normal(value) => Ok(value.to_os_string()),
        _ => Err(PackageFsError::InvalidPath),
    }
}

fn open_contained_file(parent: &Path, name: &OsStr) -> Result<Option<(File, u64)>, PackageFsError> {
    let expected = append_component(parent, name);
    let OpenedEvidence::File(file) = open_evidence_file(&expected)? else {
        return Ok(None);
    };
    confirm_contained(file, &expected)
}

fn open_evidence_file(path: &Path) -> Result<OpenedEvidence, PackageFsError> {
    let file = open_existing(path, FILE_ACCESS, SHARE_READ_WRITE, OPEN_NO_REPARSE)
        .map_err(PackageFsError::Io)?;
    classify_evidence_file(file)
}

fn classify_evidence_file(file: File) -> Result<OpenedEvidence, PackageFsError> {
    let attributes = query_attributes(&file).map_err(PackageFsError::Io)?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(PackageFsError::LinkOrReparsePoint);
    }
    if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        Ok(OpenedEvidence::NotFile)
    } else {
        Ok(OpenedEvidence::File(file))
    }
}

fn confirm_contained(file: File, expected: &Path) -> Result<Option<(File, u64)>, PackageFsError> {
    let opened = final_path(&file).map_err(PackageFsError::Io)?;
    if !windows_path_eq(&opened, expected) {
        return Err(PackageFsError::LinkOrReparsePoint);
    }
    regular_length(file)
}

fn regular_length(file: File) -> Result<Option<(File, u64)>, PackageFsError> {
    let metadata = file.metadata().map_err(PackageFsError::Io)?;
    if metadata.is_file() {
        Ok(Some((file, metadata.len())))
    } else {
        Ok(None)
    }
}

#[derive(Default)]
struct LicenseNames {
    candidates: Vec<PathBuf>,
    portable: BTreeSet<String>,
}

impl LicenseNames {
    fn finish(mut self) -> Vec<PathBuf> {
        sort_candidates(&mut self.candidates);
        self.candidates
    }
}

fn consider_license(
    root: &Path,
    entry: io::Result<std::fs::DirEntry>,
    found: &mut LicenseNames,
    max_files: u64,
) -> Result<(), PackageFsError> {
    let name = entry.map_err(PackageFsError::Io)?.file_name();
    let Some(name) = license_name(&name)? else {
        return Ok(());
    };
    if open_contained_file(root, OsStr::new(&name))?.is_none() {
        return Ok(());
    }
    push_license(found, &name, max_files)
}

fn license_name(name: &OsStr) -> Result<Option<String>, PackageFsError> {
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

fn push_license(
    found: &mut LicenseNames,
    name: &str,
    max_files: u64,
) -> Result<(), PackageFsError> {
    if !found.portable.insert(name.to_ascii_lowercase()) {
        return Ok(());
    }
    let count =
        u64::try_from(found.candidates.len()).map_or(u64::MAX, |length| length.saturating_add(1));
    if count > max_files {
        return Err(PackageFsError::TooManyFiles(count));
    }
    found.candidates.push(PathBuf::from(name));
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
