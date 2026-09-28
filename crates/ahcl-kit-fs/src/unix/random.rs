// crates/ahcl-kit-fs/src/unix/random.rs - Unix randomness for temporary names.
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

use rustix::io::Errno;

#[cfg(not(any(target_os = "android", target_os = "linux")))]
use rustix::fs::{FileType, Mode, OFlags, fstat, open};
#[cfg(not(any(target_os = "android", target_os = "linux")))]
use rustix::io as rio;
#[cfg(not(any(target_os = "android", target_os = "linux")))]
use std::path::Path;

#[cfg(any(target_os = "android", target_os = "linux"))]
pub(crate) fn fill_random(output: &mut [u8]) -> Result<(), ()> {
    fill_from_reads(output, |remaining| {
        rustix::rand::getrandom(remaining, rustix::rand::GetRandomFlags::empty())
    })
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub(crate) fn fill_random(output: &mut [u8]) -> Result<(), ()> {
    let source = open_urandom()?;
    fill_from_reads(output, |remaining| rio::read(&source, remaining))
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
fn open_urandom() -> Result<rustix::fd::OwnedFd, ()> {
    let source = open(
        Path::new("/dev/urandom"),
        OFlags::RDONLY
            .union(OFlags::NOFOLLOW)
            .union(OFlags::CLOEXEC),
        Mode::empty(),
    )
    .map_err(|_| ())?;
    let metadata = fstat(&source).map_err(|_| ())?;
    if FileType::from_raw_mode(metadata.st_mode).is_char_device() {
        Ok(source)
    } else {
        Err(())
    }
}

fn fill_from_reads(
    mut output: &mut [u8],
    mut read: impl FnMut(&mut [u8]) -> Result<usize, Errno>,
) -> Result<(), ()> {
    while !output.is_empty() {
        output = match read(output) {
            Ok(count) => advance_read(output, count)?,
            Err(Errno::INTR) => output,
            Err(_) => return Err(()),
        };
    }
    Ok(())
}

fn advance_read(output: &mut [u8], count: usize) -> Result<&mut [u8], ()> {
    if count == 0 || count > output.len() {
        return Err(());
    }
    Ok(&mut output[count..])
}
