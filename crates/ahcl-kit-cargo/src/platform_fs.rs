// crates/ahcl-kit-cargo/src/platform_fs.rs - Handle-scoped package evidence reads.
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

use std::fs::File;
use std::io::{self, Read};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(crate) use unix::{PackageDirectory, read_regular_file};
#[cfg(windows)]
pub(crate) use windows::{PackageDirectory, read_regular_file};

#[derive(Debug)]
pub(crate) enum PackageFsError {
    Io(io::Error),
    InvalidPath,
    LinkOrReparsePoint,
    PathEncoding,
    TooManyFiles(u64),
    FileTooLarge(u64),
}

impl PackageFsError {
    pub(crate) fn into_io_error(self) -> io::Error {
        match self {
            Self::Io(source) => source,
            Self::InvalidPath => io::Error::new(io::ErrorKind::InvalidInput, "invalid file path"),
            Self::LinkOrReparsePoint => io::Error::other("file path uses a link or reparse point"),
            Self::PathEncoding => io::Error::new(io::ErrorKind::InvalidData, "path is not Unicode"),
            Self::TooManyFiles(_) => io::Error::other("file count limit exceeded"),
            Self::FileTooLarge(_) => io::Error::other("file size limit exceeded"),
        }
    }
}

pub(super) fn read_file_with_limit(
    file: File,
    advertised_len: u64,
    limit: u64,
) -> Result<Vec<u8>, PackageFsError> {
    if advertised_len > limit {
        return Err(PackageFsError::FileTooLarge(advertised_len));
    }

    let capacity = usize::try_from(advertised_len).map_or(0, |length| length);
    let mut bytes = Vec::with_capacity(capacity);
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(PackageFsError::Io)?;
    let byte_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if byte_len > limit {
        return Err(PackageFsError::FileTooLarge(byte_len));
    }
    Ok(bytes)
}
