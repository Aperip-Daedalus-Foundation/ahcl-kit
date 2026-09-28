// crates/ahcl-kit-fs/src/windows/path.rs - Windows absolute-path checks.
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

use crate::safe_component::is_safe_os_component;
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf, Prefix};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FormError {
    NotAbsolute,
    Invalid,
}

pub(super) fn split_absolute(
    path: &Path,
    require_safe: bool,
) -> Result<(PathBuf, Vec<OsString>), FormError> {
    if !path.is_absolute() {
        return Err(FormError::NotAbsolute);
    }
    // Prefix, root, and normal components are accepted only in that order.
    finish_absolute_path(collect_absolute_components(path, require_safe)?)
}

struct ParsedAbsolutePath<'a> {
    prefix: Option<Prefix<'a>>,
    saw_root: bool,
    components: Vec<OsString>,
}

fn collect_absolute_components(
    path: &Path,
    require_safe: bool,
) -> Result<ParsedAbsolutePath<'_>, FormError> {
    let mut parsed = ParsedAbsolutePath {
        prefix: None,
        saw_root: false,
        components: Vec::new(),
    };
    for component in path.components() {
        accept_root_component(component, &mut parsed, require_safe)?;
    }
    Ok(parsed)
}

fn accept_root_component<'a>(
    component: Component<'a>,
    parsed: &mut ParsedAbsolutePath<'a>,
    require_safe: bool,
) -> Result<(), FormError> {
    if let Component::Prefix(value) = component {
        return accept_prefix(value.kind(), parsed);
    }
    if let Component::RootDir = component {
        return accept_root_dir(parsed);
    }
    if let Component::Normal(value) = component {
        return accept_normal(value, parsed, require_safe);
    }
    Err(FormError::Invalid)
}

fn accept_prefix<'a>(
    kind: Prefix<'a>,
    parsed: &mut ParsedAbsolutePath<'a>,
) -> Result<(), FormError> {
    if parsed.prefix.is_some() {
        return Err(FormError::Invalid);
    }
    parsed.prefix = Some(kind);
    Ok(())
}

fn accept_root_dir(parsed: &mut ParsedAbsolutePath<'_>) -> Result<(), FormError> {
    if parsed.prefix.is_none() || parsed.saw_root {
        return Err(FormError::Invalid);
    }
    parsed.saw_root = true;
    Ok(())
}

fn accept_normal(
    value: &OsStr,
    parsed: &mut ParsedAbsolutePath<'_>,
    require_safe: bool,
) -> Result<(), FormError> {
    if !normal_component_allowed(value, parsed.saw_root, require_safe) {
        return Err(FormError::Invalid);
    }
    parsed.components.push(value.to_os_string());
    Ok(())
}

fn normal_component_allowed(value: &OsStr, saw_root: bool, require_safe: bool) -> bool {
    if !saw_root {
        return false;
    }
    !require_safe || is_safe_os_component(value)
}

fn finish_absolute_path(
    parsed: ParsedAbsolutePath<'_>,
) -> Result<(PathBuf, Vec<OsString>), FormError> {
    if !parsed.saw_root {
        return Err(FormError::Invalid);
    }
    let prefix = parsed.prefix.ok_or(FormError::Invalid)?;
    let root = volume_root_for_prefix(prefix).ok_or(FormError::Invalid)?;
    Ok((root, parsed.components))
}

fn volume_root_for_prefix(prefix: Prefix<'_>) -> Option<PathBuf> {
    let mut wide = r"\\?\".encode_utf16().collect::<Vec<_>>();
    extend_volume_prefix(&mut wide, prefix)?;
    Some(PathBuf::from(OsString::from_wide(&wide)))
}

fn extend_volume_prefix(wide: &mut Vec<u16>, prefix: Prefix<'_>) -> Option<()> {
    match prefix {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => push_disk(wide, letter),
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
            push_unc(wide, server, share)
        }
        _ => None,
    }
}

fn push_disk(wide: &mut Vec<u16>, letter: u8) -> Option<()> {
    wide.push(u16::from(letter));
    wide.push(u16::from(b':'));
    wide.push(u16::from(b'\\'));
    Some(())
}

fn push_unc(wide: &mut Vec<u16>, server: &OsStr, share: &OsStr) -> Option<()> {
    wide.extend("UNC\\".encode_utf16());
    wide.extend(server.encode_wide());
    wide.push(u16::from(b'\\'));
    wide.extend(share.encode_wide());
    wide.push(u16::from(b'\\'));
    Some(())
}

pub(super) fn append_component(parent: &Path, component: &OsStr) -> PathBuf {
    let mut path = parent.to_path_buf();
    path.push(component);
    path
}

pub(super) fn windows_path_eq(opened: &Path, expected: &Path) -> bool {
    normalize_windows_path(opened) == normalize_windows_path(expected)
}

fn normalize_windows_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = text
        .strip_prefix("\\\\?\\")
        .unwrap_or(&text)
        .trim_end_matches('\\');
    text.to_ascii_lowercase()
}
