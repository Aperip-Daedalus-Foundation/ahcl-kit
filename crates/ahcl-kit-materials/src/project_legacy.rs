// crates/ahcl-kit-materials/src/project_legacy.rs - Recognize unwrapped generated LICENSE notices.
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

use crate::{MaterialsError, render};
use std::ops::Range;

pub(super) fn merge(
    existing: &str,
    desired: &str,
    has_managed_notice: bool,
) -> Result<Option<String>, MaterialsError> {
    if !has_managed_notice {
        return Ok(None);
    }
    let Some(range) = notice_range(existing)? else {
        return Ok(None);
    };
    let mut output = existing.to_owned();
    output.replace_range(range, desired);
    Ok(Some(output))
}

fn notice_range(existing: &str) -> Result<Option<Range<usize>>, MaterialsError> {
    let mut found = None;
    let mut offset = 0;
    for line in existing.split_inclusive('\n') {
        if let Some(version) = legacy_version(line) {
            if found.is_some() {
                return Err(super::ambiguous());
            }
            let length = notice_length(&existing[offset..], version)?;
            found = Some(offset..offset + length);
        }
        offset += line.len();
    }
    Ok(found)
}

fn legacy_version(line: &str) -> Option<&'static str> {
    match line.trim_end_matches(['\r', '\n']) {
        "This project is licensed under version 1.0 of the Aperip Heimdall Commons" => Some("1.0"),
        "This project is licensed under version 1.1 of the Aperip Heimdall Commons" => Some("1.1"),
        _ => None,
    }
}

fn notice_length(tail: &str, version: &str) -> Result<usize, MaterialsError> {
    for directory in directories(version) {
        let body = legacy_body(version, directory);
        if let Some(length) = exact_prefix_length(tail, &body) {
            return Ok(length + channel_length(&tail[length..], version));
        }
    }
    // A recognizable header with an edited body is not safe to replace or append to.
    Err(super::ambiguous())
}

fn directories(version: &str) -> &'static [&'static str] {
    if version == "1.0" {
        &["AHCL"]
    } else {
        &["AHCL", "licenses/AHCL", ".AHCL", ".ahcl"]
    }
}

fn legacy_body(version: &str, directory: &str) -> String {
    let filename = format!("AHCL-{version}.md");
    if version == "1.0" {
        render::v1_0_license_notice(directory, &filename)
    } else {
        render::v1_1_license_notice(directory, &filename)
    }
}

fn exact_prefix_length(tail: &str, expected: &str) -> Option<usize> {
    if tail.starts_with(expected) {
        return Some(expected.len());
    }
    let crlf = expected.replace('\n', "\r\n");
    tail.starts_with(&crlf).then_some(crlf.len())
}

fn channel_length(tail: &str, version: &str) -> usize {
    let normalized = tail.replace("\r\n", "\n");
    let Some(channel) =
        normalized.strip_prefix("\nChannels for Non-AHCL Special Authorizations:\n")
    else {
        return 0;
    };
    let channel = channel.lines().next().unwrap_or_default();
    let section = render::special_authorization_section(channel, version);
    exact_prefix_length(tail, &section).unwrap_or(0)
}
