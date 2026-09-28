// crates/ahcl-kit-config/src/schema_glob.rs - Glob compilation and matching steps.
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

use super::{ClassMember, GlobPattern, GlobToken, is_separator};

pub(super) fn push_glob_token(
    characters: &[char],
    tokens: &mut Vec<GlobToken>,
    index: usize,
    package_name: bool,
) -> Result<usize, String> {
    match characters[index] {
        '*' => push_star(characters, tokens, index, package_name),
        '?' => {
            tokens.push(GlobToken::Any);
            Ok(index + 1)
        }
        '[' => push_class(characters, tokens, index),
        character => {
            tokens.push(GlobToken::Literal(character));
            Ok(index + 1)
        }
    }
}

fn push_star(
    characters: &[char],
    tokens: &mut Vec<GlobToken>,
    index: usize,
    package_name: bool,
) -> Result<usize, String> {
    if package_name && characters.get(index + 1) == Some(&'*') {
        push_unique_star(tokens, GlobToken::Globstar);
        return Ok(index + 2);
    }
    push_unique_star(tokens, GlobToken::Star);
    Ok(index + 1)
}

fn push_unique_star(tokens: &mut Vec<GlobToken>, token: GlobToken) {
    let duplicate = matches!(
        (&token, tokens.last()),
        (GlobToken::Globstar, Some(GlobToken::Globstar)) | (GlobToken::Star, Some(GlobToken::Star))
    );
    if !duplicate {
        tokens.push(token);
    }
}

fn push_class(
    characters: &[char],
    tokens: &mut Vec<GlobToken>,
    index: usize,
) -> Result<usize, String> {
    let close = class_close(characters, index)?;
    let (negative, member_index) = class_start(characters, index, close)?;
    let members = class_members(characters, member_index, close);
    tokens.push(GlobToken::Class { negative, members });
    Ok(close + 1)
}

fn class_close(characters: &[char], index: usize) -> Result<usize, String> {
    let Some(close) = characters[index + 1..]
        .iter()
        .position(|character| *character == ']')
    else {
        return Err("unterminated character class".to_owned());
    };
    Ok(index + 1 + close)
}

fn class_start(characters: &[char], index: usize, close: usize) -> Result<(bool, usize), String> {
    let mut member_index = index + 1;
    let negative = matches!(characters.get(member_index), Some('!' | '^'));
    if negative {
        member_index += 1;
    }
    if member_index == close {
        return Err("empty character class".to_owned());
    }
    Ok((negative, member_index))
}

fn class_members(characters: &[char], mut member_index: usize, close: usize) -> Vec<ClassMember> {
    let mut members = Vec::new();
    while member_index < close {
        member_index = push_class_member(characters, &mut members, member_index, close);
    }
    members
}

fn push_class_member(
    characters: &[char],
    members: &mut Vec<ClassMember>,
    member_index: usize,
    close: usize,
) -> usize {
    if member_index + 2 < close && characters[member_index + 1] == '-' {
        members.push(ClassMember::Range(
            characters[member_index],
            characters[member_index + 2],
        ));
        return member_index + 3;
    }
    members.push(ClassMember::Single(characters[member_index]));
    member_index + 1
}

pub(super) fn advance_glob(
    pattern: &GlobPattern,
    token: &GlobToken,
    characters: &[char],
    previous: &[bool],
) -> Vec<bool> {
    let mut current = vec![false; characters.len() + 1];
    match token {
        GlobToken::Star | GlobToken::Globstar => {
            advance_star(pattern, token, characters, previous, &mut current);
        }
        token => advance_token(
            token,
            characters,
            !pattern.package_name,
            previous,
            &mut current,
        ),
    }
    current
}

fn advance_star(
    pattern: &GlobPattern,
    token: &GlobToken,
    characters: &[char],
    previous: &[bool],
    current: &mut [bool],
) {
    let crosses_separators = pattern.package_name || matches!(token, GlobToken::Globstar);
    current[0] = previous[0];
    for index in 1..=characters.len() {
        let allowed = crosses_separators || !is_separator(characters[index - 1]);
        current[index] = previous[index] || (allowed && current[index - 1]);
    }
}

fn advance_token(
    token: &GlobToken,
    characters: &[char],
    separators: bool,
    previous: &[bool],
    current: &mut [bool],
) {
    for index in 1..=characters.len() {
        current[index] =
            previous[index - 1] && token.matches_character(characters[index - 1], separators);
    }
}
