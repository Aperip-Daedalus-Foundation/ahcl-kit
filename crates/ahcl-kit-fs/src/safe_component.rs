// crates/ahcl-kit-fs/src/safe_component.rs - Shared single-component path checks.
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

#[cfg(windows)]
use std::ffi::OsStr;

/// Reject empty, reserved, and separator-bearing path components.
///
/// Evidence reads keep their own normal-component parser. This check is the
/// project-tree rule shared by relative material paths and Windows root opens.
pub fn is_safe_component(component: &str) -> bool {
    shape_is_safe(component) && !reserved_device(component)
}

#[cfg(windows)]
pub(crate) fn is_safe_os_component(component: &OsStr) -> bool {
    match component.to_str() {
        Some(value) => is_safe_component(value),
        None => false,
    }
}

fn shape_is_safe(component: &str) -> bool {
    if component.is_empty() || component == "." || component == ".." {
        return false;
    }
    if component.as_bytes().contains(&0) || component.ends_with([' ', '.']) {
        return false;
    }
    component
        .chars()
        .all(|character| !forbidden_char(character))
}

fn forbidden_char(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\'
        )
}

fn reserved_device(component: &str) -> bool {
    let base = device_base(component);
    if is_legacy_device(&base) {
        return true;
    }
    match com_or_lpt_suffix(&base) {
        Some(suffix) => is_device_suffix(suffix),
        None => false,
    }
}

fn device_base(component: &str) -> String {
    let base = match component.split_once('.') {
        Some((name, _)) => name,
        None => component,
    };
    base.to_ascii_uppercase()
}

fn is_legacy_device(base: &str) -> bool {
    base == "CON" || base == "PRN" || base == "AUX" || base == "NUL"
}

fn com_or_lpt_suffix(base: &str) -> Option<&str> {
    base.strip_prefix("COM")
        .or_else(|| base.strip_prefix("LPT"))
}

fn is_device_suffix(suffix: &str) -> bool {
    is_ascii_device_index(suffix) || is_superscript_device_index(suffix)
}

fn is_ascii_device_index(suffix: &str) -> bool {
    let mut chars = suffix.chars();
    let Some(character) = chars.next() else {
        return false;
    };
    chars.next().is_none() && character.is_ascii_digit() && character != '0'
}

fn is_superscript_device_index(suffix: &str) -> bool {
    suffix == "\u{b9}" || suffix == "\u{b2}" || suffix == "\u{b3}"
}
