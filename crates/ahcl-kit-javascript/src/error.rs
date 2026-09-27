// crates/ahcl-kit-javascript/src/error.rs - JavaScript adapter errors.
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

use ahcl_kit_core::RepoPath;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum JavascriptError {
    ManifestInvalid { manifest: RepoPath },
    LockfileMissing { manifest: RepoPath },
    LockfileAmbiguous { manifest: RepoPath, found: String },
    LockfileOutsideProject { path: String },
    LockfileRead { path: PathBuf },
    LockfileTooLarge { path: RepoPath },
    LockfileParse { path: RepoPath, message: String },
    BinaryBunLockfile { path: RepoPath },
    PackageSelection { package: String },
    DuplicatePackage { package_id: String },
    PathInvalid { path: String },
}

impl JavascriptError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ManifestInvalid { .. } => "javascript.manifest_invalid",
            Self::LockfileMissing { .. } => "javascript.lockfile_missing",
            Self::LockfileAmbiguous { .. } => "javascript.lockfile_ambiguous",
            Self::LockfileOutsideProject { .. } => "javascript.lockfile_outside_project",
            Self::LockfileRead { .. } => "javascript.lockfile_read",
            Self::LockfileTooLarge { .. } => "javascript.lockfile_too_large",
            Self::LockfileParse { .. } => "javascript.lockfile_parse",
            Self::BinaryBunLockfile { .. } => "javascript.bun_lockfile_binary",
            Self::PackageSelection { .. } => "javascript.package_selection",
            Self::DuplicatePackage { .. } => "javascript.package_duplicate",
            Self::PathInvalid { .. } => "javascript.path_invalid",
        }
    }
}

impl fmt::Display for JavascriptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ManifestInvalid { manifest } => {
                write!(
                    formatter,
                    "JavaScript manifest is not a safe regular file: {manifest}"
                )
            }
            Self::LockfileMissing { manifest } => {
                write!(
                    formatter,
                    "JavaScript manifest {manifest} has no supported lockfile"
                )
            }
            Self::LockfileAmbiguous { manifest, found } => {
                write!(
                    formatter,
                    "JavaScript manifest {manifest} has multiple lockfiles ({found}); set javascript.managers"
                )
            }
            Self::LockfileOutsideProject { .. } => {
                formatter.write_str("JavaScript lockfile is outside the project root")
            }
            Self::LockfileRead { .. } => {
                formatter.write_str("JavaScript lockfile could not be read")
            }
            Self::LockfileTooLarge { path } => {
                write!(
                    formatter,
                    "JavaScript lockfile exceeds the size limit: {path}"
                )
            }
            Self::LockfileParse { path, message } => {
                write!(
                    formatter,
                    "JavaScript lockfile {path} could not be parsed: {message}"
                )
            }
            Self::BinaryBunLockfile { path } => {
                write!(
                    formatter,
                    "binary Bun lockfile {path} is not supported; generate text bun.lock"
                )
            }
            Self::PackageSelection { package } => {
                write!(
                    formatter,
                    "JavaScript package selection did not match a workspace root: {package}"
                )
            }
            Self::DuplicatePackage { package_id } => {
                write!(
                    formatter,
                    "JavaScript resolution produced a duplicate package id: {package_id}"
                )
            }
            Self::PathInvalid { .. } => {
                formatter.write_str("JavaScript path cannot be represented safely")
            }
        }
    }
}

impl Error for JavascriptError {}
