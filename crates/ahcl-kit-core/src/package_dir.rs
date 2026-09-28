// crates/ahcl-kit-core/src/package_dir.rs - Shared third-party directory names.
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

use crate::sha256_hex;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageDirectoryInput {
    package_id: String,
    name: String,
    version: String,
}

impl PackageDirectoryInput {
    pub fn new(
        package_id: impl Into<String>,
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            package_id: package_id.into(),
            name: name.into(),
            version: version.into(),
        }
    }

    pub fn package_id(&self) -> &str {
        &self.package_id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }
}

pub fn assign_package_directories(packages: &[PackageDirectoryInput]) -> BTreeMap<String, String> {
    let mut bases = BTreeMap::new();
    let mut collision_groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for package in packages {
        let base = format!(
            "{}-{}",
            encode_segment(package.name()),
            encode_segment(package.version())
        );
        bases.insert(package.package_id().to_owned(), base.clone());
        collision_groups
            .entry(base.to_ascii_lowercase())
            .or_default()
            .insert(package.package_id().to_owned());
    }

    bases
        .into_iter()
        .map(|(package_id, base)| {
            let collides = collision_groups
                .get(&base.to_ascii_lowercase())
                .is_some_and(|ids| ids.len() > 1);
            if collides {
                let digest = sha256_hex(package_id.as_bytes());
                (package_id, format!("{base}-{}", &digest[..8]))
            } else {
                (package_id, base)
            }
        })
        .collect()
}

fn encode_segment(value: &str) -> String {
    let invalid_tail = value.ends_with(['.', ' ']);
    let encoded = encode_bytes(value, invalid_tail);
    // Reserved, empty, and trailing-dot names are prefixed after encoding so
    // the Windows device-name check stays outside the byte loop.
    if value.is_empty() || invalid_tail || is_windows_reserved(value) {
        format!("_pkg_{encoded}")
    } else {
        encoded
    }
}

fn encode_bytes(value: &str, invalid_tail: bool) -> String {
    let mut encoded = String::new();
    for (index, byte) in value.as_bytes().iter().copied().enumerate() {
        push_encoded_byte(
            &mut encoded,
            byte,
            encode_trailing_byte(invalid_tail, index, value.len(), byte),
        );
    }
    encoded
}

fn encode_trailing_byte(invalid_tail: bool, index: usize, len: usize, byte: u8) -> bool {
    invalid_tail && index + 1 == len && is_dot_or_space(byte)
}

fn is_dot_or_space(byte: u8) -> bool {
    matches!(byte, b'.' | b' ')
}

fn push_encoded_byte(encoded: &mut String, byte: u8, encode_trailing: bool) {
    if encode_trailing || !is_plain_segment_byte(byte) {
        push_hex_escape(encoded, byte);
        return;
    }
    encoded.push(char::from(byte));
}

fn is_plain_segment_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || is_plain_punctuation(byte)
}

fn is_plain_punctuation(byte: u8) -> bool {
    matches!(byte, b'.' | b'_' | b'-')
}

fn push_hex_escape(encoded: &mut String, byte: u8) {
    encoded.push('_');
    encoded.push(upper_hex_digit(byte >> 4));
    encoded.push(upper_hex_digit(byte & 0x0f));
}

fn is_windows_reserved(value: &str) -> bool {
    let base = value
        .split_once('.')
        .map_or(value, |(name, _)| name)
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();
    if matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    base.strip_prefix("COM")
        .or_else(|| base.strip_prefix("LPT"))
        .is_some_and(|number| {
            matches!(
                number,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
}

fn upper_hex_digit(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0' + value),
        _ => char::from(b'A' + value - 10),
    }
}
