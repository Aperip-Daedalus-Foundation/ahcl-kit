// crates/ahcl-kit-config/src/parser/scope_block.rs - Literal covered-scope blocks.
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

use super::{ConfigError, Line, ScalarValue, Value, parse_error};

pub(super) fn parse(
    lines: &[Line<'_>],
    header: usize,
    key: &str,
) -> Result<(Value, usize), ConfigError> {
    if key != "covered-scope" {
        return Err(parse_error(
            lines[header].number,
            "literal text blocks are supported only for covered-scope",
        ));
    }
    let (text, next_index) = read_body(lines, header + 1);
    if text.trim().is_empty() {
        return Err(parse_error(
            lines[header].number,
            "covered-scope text block must contain text indented by two spaces",
        ));
    }
    Ok((
        Value::Scalar(ScalarValue::String(text.trim_matches('\n').to_owned())),
        next_index,
    ))
}

fn read_body(lines: &[Line<'_>], mut index: usize) -> (String, usize) {
    let mut text = String::new();
    while let Some(line) = lines.get(index) {
        let Some(content) = body_line(line.content) else {
            break;
        };
        text.push_str(content);
        text.push('\n');
        index += 1;
    }
    (text, index)
}

fn body_line(content: &str) -> Option<&str> {
    if content.trim().is_empty() {
        Some("")
    } else {
        content.strip_prefix("  ")
    }
}
