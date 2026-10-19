// crates/ahcl-kit-materials/src/history_text.rs - Literal history text in Markdown.
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

use std::fmt::Write;

pub(crate) const MARKER: &str = "<!-- AHCL KIT HISTORY TEXT: literal-v1 -->";

pub(crate) fn literal(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for line in value.split_inclusive('\n') {
        push_line(&mut output, line);
    }
    output
}

fn push_line(output: &mut String, line: &str) {
    let block = block_prefix(line);
    for (offset, character) in line.char_indices() {
        if "&<>\\`*_[]!#~|+".contains(character) || block == Some(offset) {
            let _ = write!(output, "&#{};", u32::from(character));
        } else {
            output.push(character);
        }
    }
}

fn block_prefix(line: &str) -> Option<usize> {
    let value = line.trim_start();
    let is_block = value.starts_with(['-', '+', '=']) || ordered_list(value);
    is_block.then_some(line.len() - value.len())
}

fn ordered_list(value: &str) -> bool {
    let Some((number, tail)) = value.split_once(['.', ')']) else {
        return false;
    };
    !number.is_empty()
        && number.bytes().all(|byte| byte.is_ascii_digit())
        && tail.starts_with(char::is_whitespace)
}

pub(crate) fn normalized(document: &str, value: &str) -> String {
    let value = value.replace("\r\n", "\n");
    if document.lines().any(|line| line == MARKER) {
        decode(&value)
    } else {
        value
    }
}

// Decode exactly once, and only for documents marked as our literal format.
// Unmarked signed snapshots retain their original Markdown and entity spelling.
fn decode(value: &str) -> String {
    let mut remaining = value;
    let mut output = String::with_capacity(value.len());
    while let Some((prefix, suffix)) = remaining.split_once("&#") {
        output.push_str(prefix);
        if let Some((character, tail)) = reference(suffix) {
            output.push(character);
            remaining = tail;
        } else {
            output.push_str("&#");
            remaining = suffix;
        }
    }
    output.push_str(remaining);
    output
}

fn reference(value: &str) -> Option<(char, &str)> {
    let (number, tail) = value.split_once(';')?;
    let character = char::from_u32(number.parse().ok()?)?;
    character.is_ascii().then_some((character, tail))
}
