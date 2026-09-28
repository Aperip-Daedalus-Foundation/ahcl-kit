// crates/ahcl-kit-javascript/src/error.rs - JavaScript adapter errors.
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
            Self::ManifestInvalid { .. }
            | Self::LockfileMissing { .. }
            | Self::LockfileAmbiguous { .. }
            | Self::LockfileOutsideProject { .. }
            | Self::LockfileRead { .. }
            | Self::LockfileTooLarge { .. } => lockfile_group_code(self),
            other => selection_group_code(other),
        }
    }
}

fn lockfile_group_code(error: &JavascriptError) -> &'static str {
    match error {
        JavascriptError::ManifestInvalid { .. } => "javascript.manifest_invalid",
        JavascriptError::LockfileMissing { .. } => "javascript.lockfile_missing",
        JavascriptError::LockfileAmbiguous { .. } => "javascript.lockfile_ambiguous",
        JavascriptError::LockfileOutsideProject { .. } => "javascript.lockfile_outside_project",
        JavascriptError::LockfileRead { .. } => "javascript.lockfile_read",
        JavascriptError::LockfileTooLarge { .. } => "javascript.lockfile_too_large",
        // Exhaustive for variants routed to `selection_group_code`.
        JavascriptError::LockfileParse { .. }
        | JavascriptError::BinaryBunLockfile { .. }
        | JavascriptError::PackageSelection { .. }
        | JavascriptError::DuplicatePackage { .. }
        | JavascriptError::PathInvalid { .. } => "javascript.lockfile_parse",
    }
}

fn selection_group_code(error: &JavascriptError) -> &'static str {
    match error {
        JavascriptError::LockfileParse { .. } => "javascript.lockfile_parse",
        JavascriptError::BinaryBunLockfile { .. } => "javascript.bun_lockfile_binary",
        JavascriptError::PackageSelection { .. } => "javascript.package_selection",
        JavascriptError::DuplicatePackage { .. } => "javascript.package_duplicate",
        JavascriptError::PathInvalid { .. } => "javascript.path_invalid",
        // Exhaustive for variants routed to `lockfile_group_code`.
        JavascriptError::ManifestInvalid { .. }
        | JavascriptError::LockfileMissing { .. }
        | JavascriptError::LockfileAmbiguous { .. }
        | JavascriptError::LockfileOutsideProject { .. }
        | JavascriptError::LockfileRead { .. }
        | JavascriptError::LockfileTooLarge { .. } => "javascript.manifest_invalid",
    }
}

impl fmt::Display for JavascriptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ManifestInvalid { .. }
            | Self::LockfileMissing { .. }
            | Self::LockfileAmbiguous { .. }
            | Self::LockfileOutsideProject { .. }
            | Self::LockfileRead { .. }
            | Self::LockfileTooLarge { .. } => fmt_lockfile_group(self, formatter),
            other => fmt_selection_group(other, formatter),
        }
    }
}

fn fmt_lockfile_group(error: &JavascriptError, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    match error {
        JavascriptError::ManifestInvalid { manifest } => write!(
            formatter,
            "JavaScript manifest is not a safe regular file: {manifest}"
        ),
        JavascriptError::LockfileMissing { manifest } => write!(
            formatter,
            "JavaScript manifest {manifest} has no supported lockfile"
        ),
        JavascriptError::LockfileAmbiguous { manifest, found } => write!(
            formatter,
            "JavaScript manifest {manifest} has multiple lockfiles ({found}); set javascript.managers"
        ),
        JavascriptError::LockfileOutsideProject { .. } => {
            formatter.write_str("JavaScript lockfile is outside the project root")
        }
        JavascriptError::LockfileRead { .. } => {
            formatter.write_str("JavaScript lockfile could not be read")
        }
        JavascriptError::LockfileTooLarge { path } => {
            write!(
                formatter,
                "JavaScript lockfile exceeds the size limit: {path}"
            )
        }
        JavascriptError::LockfileParse { .. }
        | JavascriptError::BinaryBunLockfile { .. }
        | JavascriptError::PackageSelection { .. }
        | JavascriptError::DuplicatePackage { .. }
        | JavascriptError::PathInvalid { .. } => Ok(()),
    }
}

fn fmt_selection_group(error: &JavascriptError, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    match error {
        JavascriptError::LockfileParse { path, message } => write!(
            formatter,
            "JavaScript lockfile {path} could not be parsed: {message}"
        ),
        JavascriptError::BinaryBunLockfile { path } => write!(
            formatter,
            "binary Bun lockfile {path} is not supported; generate text bun.lock"
        ),
        JavascriptError::PackageSelection { package } => write!(
            formatter,
            "JavaScript package selection did not match a workspace root: {package}"
        ),
        JavascriptError::DuplicatePackage { package_id } => write!(
            formatter,
            "JavaScript resolution produced a duplicate package id: {package_id}"
        ),
        JavascriptError::PathInvalid { .. } => {
            formatter.write_str("JavaScript path cannot be represented safely")
        }
        JavascriptError::ManifestInvalid { .. }
        | JavascriptError::LockfileMissing { .. }
        | JavascriptError::LockfileAmbiguous { .. }
        | JavascriptError::LockfileOutsideProject { .. }
        | JavascriptError::LockfileRead { .. }
        | JavascriptError::LockfileTooLarge { .. } => Ok(()),
    }
}

impl Error for JavascriptError {}
