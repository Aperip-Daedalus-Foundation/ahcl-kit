// crates/ahcl-kit-cargo/src/limits.rs - Evidence limits, hashing, and package directory assignment.
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

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_EVIDENCE_FILE_BYTES: u64 = 2_097_152;
pub const MAX_EVIDENCE_FILES_PER_PACKAGE: u64 = 64;
pub const MAX_AGGREGATE_EVIDENCE_BYTES: u64 = 536_870_912;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceLimits {
    max_file_bytes: u64,
    max_files_per_package: u64,
    max_aggregate_bytes: u64,
}

impl EvidenceLimits {
    pub const fn new(
        max_file_bytes: u64,
        max_files_per_package: u64,
        max_aggregate_bytes: u64,
    ) -> Self {
        Self {
            max_file_bytes,
            max_files_per_package,
            max_aggregate_bytes,
        }
    }

    pub const fn max_file_bytes(self) -> u64 {
        self.max_file_bytes
    }

    pub const fn max_files_per_package(self) -> u64 {
        self.max_files_per_package
    }

    pub const fn max_aggregate_bytes(self) -> u64 {
        self.max_aggregate_bytes
    }
}

impl Default for EvidenceLimits {
    fn default() -> Self {
        Self::new(
            MAX_EVIDENCE_FILE_BYTES,
            MAX_EVIDENCE_FILES_PER_PACKAGE,
            MAX_AGGREGATE_EVIDENCE_BYTES,
        )
    }
}

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
    let reserved = is_windows_reserved(value);
    let mut encoded = String::new();
    for (index, byte) in value.as_bytes().iter().copied().enumerate() {
        let encode_trailing =
            invalid_tail && index + 1 == value.len() && matches!(byte, b'.' | b' ');
        if !encode_trailing && (byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            encoded.push(char::from(byte));
        } else {
            encoded.push('_');
            encoded.push(upper_hex_digit(byte >> 4));
            encoded.push(upper_hex_digit(byte & 0x0f));
        }
    }
    if value.is_empty() || invalid_tail || reserved {
        format!("_pkg_{encoded}")
    } else {
        encoded
    }
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

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut result = String::with_capacity(64);
    for byte in digest {
        result.push(hex_digit(byte >> 4));
        result.push(hex_digit(byte & 0x0f));
    }
    result
}

fn hex_digit(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0' + value),
        _ => char::from(b'a' + value - 10),
    }
}

fn upper_hex_digit(value: u8) -> char {
    match value {
        0..=9 => char::from(b'0' + value),
        _ => char::from(b'A' + value - 10),
    }
}
