// crates/ahcl-kit-fs/src/failure.rs - Language-neutral filesystem outcomes.
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

/// Why an absolute project root could not be opened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootFailure {
    NotAbsolute,
    Invalid,
    FilesystemRoot,
    Open,
    Reparse,
    NotDirectory,
}

/// Why a relative walk inside an opened root failed.
///
/// `Internal` is an invariant failure and is not tied to the requested path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathFailure {
    Reparse,
    NotDirectory,
    Io,
    Internal,
}

/// Bytes read without following a link, or a non-file node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReadNode {
    Absent,
    Other,
    File(Vec<u8>),
}

/// Why an atomic create or replace failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteFailure {
    Path(PathFailure),
    Commit,
    Write,
    TempCreate,
}

/// Why a verified removal failed.
///
/// Platform backends choose the variant. A Unix walk reports `Path`, while a
/// Windows managed walk reports the managed outcome directly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoveFailure {
    Path(PathFailure),
    ManagedLink,
    ManagedTree,
    ManagedRemove,
}

/// Result of a removal that did not fail closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoveStatus {
    Removed,
    Absent,
    Rejected,
}

/// Failure while scanning a directory that was already opened no-follow.
#[derive(Debug)]
pub enum ScanFailure<E> {
    Io,
    TooMany,
    Visit(E),
}

/// One child of an opened directory.
///
/// `Directory` carries the opened child so a later listing does not look the
/// name up a second time.
#[derive(Debug)]
pub enum Inspected<D> {
    File,
    Link,
    Other,
    Directory(D),
}
