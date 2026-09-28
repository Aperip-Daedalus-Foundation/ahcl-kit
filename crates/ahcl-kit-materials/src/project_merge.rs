// crates/ahcl-kit-materials/src/project_merge.rs - Managed AHCL block merge.
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

use super::{MaterialsError, MaterialsErrorCode};
use ahcl_kit_core::{ChangePlan, ProjectEntry, ProjectView, RepoPath};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub(super) enum ManagedFileKind {
    License,
    Document,
    Adoption,
    Official,
    Dependency,
}

#[derive(Clone, Debug)]
struct ManagedRange {
    start: usize,
    end: usize,
    key: String,
}

struct SpliceInput<'a> {
    existing: &'a str,
    desired: &'a str,
    kind: ManagedFileKind,
    ranges: &'a [ManagedRange],
    desired_ranges: &'a [ManagedRange],
    desired_by_key: &'a BTreeMap<String, &'a str>,
}

struct ScopeScan<'a> {
    content: &'a str,
    line: &'a str,
    offset: usize,
}

pub(super) fn write_managed(
    view: &dyn ProjectView,
    plan: &mut ChangePlan,
    path: RepoPath,
    generated: &[String],
    kind: ManagedFileKind,
) -> Result<(), MaterialsError> {
    if generated.is_empty() {
        return Ok(());
    }
    let Some(bytes) = managed_replacement(view, &path, generated, kind)? else {
        return Ok(());
    };
    super::write(plan, path, bytes)
}

fn managed_replacement(
    view: &dyn ProjectView,
    path: &RepoPath,
    generated: &[String],
    kind: ManagedFileKind,
) -> Result<Option<Vec<u8>>, MaterialsError> {
    let desired = generated.join("\n");
    match view
        .entry(path)
        .map_err(|_| MaterialsError::new(MaterialsErrorCode::View))?
    {
        ProjectEntry::Absent => Ok(Some(desired.into_bytes())),
        ProjectEntry::File(existing) => merged_replacement(existing, &desired, kind),
        ProjectEntry::Other => Err(MaterialsError::new(MaterialsErrorCode::Plan)),
    }
}

fn merged_replacement(
    existing: Vec<u8>,
    desired: &str,
    kind: ManagedFileKind,
) -> Result<Option<Vec<u8>>, MaterialsError> {
    let existing = String::from_utf8(existing).map_err(|_| ambiguous())?;
    Ok(merge_managed(&existing, desired, kind)?.map(String::into_bytes))
}

fn merge_managed(
    existing: &str,
    desired: &str,
    kind: ManagedFileKind,
) -> Result<Option<String>, MaterialsError> {
    if skips_managed_merge(kind) {
        return Ok(None);
    }
    let desired_ranges = managed_ranges(desired, kind)?;
    let ranges = managed_ranges(existing, kind)?;
    merge_range_sets(existing, desired, kind, &ranges, &desired_ranges)
}

fn skips_managed_merge(kind: ManagedFileKind) -> bool {
    matches!(
        kind,
        ManagedFileKind::Official | ManagedFileKind::Dependency
    )
}

fn is_license_kind(kind: ManagedFileKind) -> bool {
    matches!(kind, ManagedFileKind::License)
}

fn merge_range_sets(
    existing: &str,
    desired: &str,
    kind: ManagedFileKind,
    ranges: &[ManagedRange],
    desired_ranges: &[ManagedRange],
) -> Result<Option<String>, MaterialsError> {
    if desired_ranges.is_empty() {
        return Ok(None);
    }
    if ranges.is_empty() {
        return Ok(Some(append_fresh_block(existing, desired)));
    }
    let output = splice_managed(existing, desired, kind, ranges, desired_ranges)?;
    Ok(changed_output(existing, output))
}

fn append_fresh_block(existing: &str, desired: &str) -> String {
    let separator = if existing.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{existing}{separator}{desired}")
}

fn changed_output(existing: &str, output: String) -> Option<String> {
    if output == existing {
        None
    } else {
        Some(output)
    }
}

fn splice_managed(
    existing: &str,
    desired: &str,
    kind: ManagedFileKind,
    ranges: &[ManagedRange],
    desired_ranges: &[ManagedRange],
) -> Result<String, MaterialsError> {
    let desired_by_key = desired_key_index(desired, desired_ranges)?;
    let existing_keys = existing_key_index(ranges)?;
    let input = SpliceInput {
        existing,
        desired,
        kind,
        ranges,
        desired_ranges,
        desired_by_key: &desired_by_key,
    };
    let mut output = String::with_capacity(existing.len() + desired.len());
    splice_existing(&mut output, &input)?;
    append_missing_ranges(&mut output, desired, desired_ranges, &existing_keys);
    Ok(output)
}

fn desired_key_index<'a>(
    desired: &'a str,
    ranges: &[ManagedRange],
) -> Result<BTreeMap<String, &'a str>, MaterialsError> {
    let mut indexed = BTreeMap::new();
    for range in ranges {
        let value = desired.get(range.start..range.end).ok_or_else(ambiguous)?;
        if indexed.insert(range.key.clone(), value).is_some() {
            return Err(ambiguous());
        }
    }
    Ok(indexed)
}

fn existing_key_index(ranges: &[ManagedRange]) -> Result<BTreeSet<String>, MaterialsError> {
    let mut keys = BTreeSet::new();
    for range in ranges {
        if !keys.insert(range.key.clone()) {
            return Err(ambiguous());
        }
    }
    Ok(keys)
}

fn splice_existing(output: &mut String, input: &SpliceInput<'_>) -> Result<(), MaterialsError> {
    let mut cursor = 0;
    for range in input.ranges {
        output.push_str(&input.existing[cursor..range.start]);
        splice_one(output, input, range)?;
        cursor = range.end;
    }
    output.push_str(&input.existing[cursor..]);
    Ok(())
}

fn splice_one(
    output: &mut String,
    input: &SpliceInput<'_>,
    range: &ManagedRange,
) -> Result<(), MaterialsError> {
    let old = &input.existing[range.start..range.end];
    if let Some(replacement) = input.desired_by_key.get(&range.key) {
        push_replacement(output, input, range, old, replacement)?;
    } else {
        output.push_str(old);
    }
    Ok(())
}

fn push_replacement(
    output: &mut String,
    input: &SpliceInput<'_>,
    range: &ManagedRange,
    old: &str,
    replacement: &str,
) -> Result<(), MaterialsError> {
    if matches!(input.kind, ManagedFileKind::Adoption) && !old.contains("Effective Date:") {
        push_adoption_event(output, input, range, old)?;
    } else {
        output.push_str(replacement);
    }
    Ok(())
}

// Historical adoption blocks without an effective date keep their record and
// gain only the new event. Later blocks are replaced as a whole.
fn push_adoption_event(
    output: &mut String,
    input: &SpliceInput<'_>,
    range: &ManagedRange,
    old: &str,
) -> Result<(), MaterialsError> {
    let end_marker = old
        .rfind("<!-- END AHCL KIT MANAGED SCOPE:")
        .ok_or_else(ambiguous)?;
    let desired_range = input
        .desired_ranges
        .iter()
        .find(|candidate| candidate.key == range.key)
        .ok_or_else(|| MaterialsError::new(MaterialsErrorCode::Plan))?;
    let desired_block = input
        .desired
        .get(desired_range.start..desired_range.end)
        .ok_or_else(|| MaterialsError::new(MaterialsErrorCode::Plan))?;
    let event = adoption_event(desired_block);
    output.push_str(&old[..end_marker]);
    if !old.contains(event) {
        output.push('\n');
        output.push_str(event);
        output.push('\n');
    }
    output.push_str(&old[end_marker..]);
    Ok(())
}

fn adoption_event(desired_block: &str) -> &str {
    let event_start = desired_block
        .find('\n')
        .map(|index| index + 1)
        .unwrap_or(desired_block.len());
    let event_end = desired_block
        .rfind("<!-- END AHCL KIT MANAGED SCOPE:")
        .unwrap_or(desired_block.len());
    desired_block[event_start..event_end].trim()
}

fn append_missing_ranges(
    output: &mut String,
    desired: &str,
    desired_ranges: &[ManagedRange],
    existing_keys: &BTreeSet<String>,
) {
    for range in desired_ranges {
        if !existing_keys.contains(&range.key) {
            append_missing_range(output, desired, range);
        }
    }
}

fn append_missing_range(output: &mut String, desired: &str, range: &ManagedRange) {
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push('\n');
    output.push_str(&desired[range.start..range.end]);
}

// Managed blocks are recognized only by their exact begin and end lines.
// A license notice may carry its scope key on an inner marker line.
fn managed_ranges(
    content: &str,
    kind: ManagedFileKind,
) -> Result<Vec<ManagedRange>, MaterialsError> {
    let mut ranges = Vec::new();
    let mut active: Option<(usize, String)> = None;
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        observe_managed_line(content, kind, line, &mut ranges, &mut active, &mut offset)?;
    }
    if active.is_some() {
        return Err(ambiguous());
    }
    Ok(ranges)
}

fn observe_managed_line(
    content: &str,
    kind: ManagedFileKind,
    line: &str,
    ranges: &mut Vec<ManagedRange>,
    active: &mut Option<(usize, String)>,
    offset: &mut usize,
) -> Result<(), MaterialsError> {
    let text = trimmed_line(line);
    if line_begins_scope(text, kind) {
        begin_scope(text, kind, *offset, active)?;
    } else if line_ends_scope(text, kind) {
        let scan = ScopeScan {
            content,
            line,
            offset: *offset,
        };
        end_scope(scan, text, kind, ranges, active)?;
    }
    *offset += line.len();
    Ok(())
}

fn trimmed_line(line: &str) -> &str {
    line.strip_suffix('\n')
        .unwrap_or(line)
        .trim_end_matches('\r')
}

fn line_begins_scope(text: &str, kind: ManagedFileKind) -> bool {
    if matches!(kind, ManagedFileKind::License) {
        text == "----- BEGIN AHCL NOTICE -----"
    } else {
        text.starts_with("<!-- BEGIN AHCL KIT MANAGED SCOPE: ") && text.ends_with(" -->")
    }
}

fn line_ends_scope(text: &str, kind: ManagedFileKind) -> bool {
    if matches!(kind, ManagedFileKind::License) {
        text == "----- END AHCL NOTICE -----"
    } else {
        text.starts_with("<!-- END AHCL KIT MANAGED SCOPE: ") && text.ends_with(" -->")
    }
}

fn begin_scope(
    text: &str,
    kind: ManagedFileKind,
    offset: usize,
    active: &mut Option<(usize, String)>,
) -> Result<(), MaterialsError> {
    if active.is_some() {
        return Err(ambiguous());
    }
    *active = Some((offset, scope_key_from_begin(text, kind)));
    Ok(())
}

fn scope_key_from_begin(text: &str, kind: ManagedFileKind) -> String {
    if matches!(kind, ManagedFileKind::License) {
        String::new()
    } else {
        text.trim_start_matches("<!-- BEGIN AHCL KIT MANAGED SCOPE: ")
            .trim_end_matches(" -->")
            .to_owned()
    }
}

fn end_scope(
    scan: ScopeScan<'_>,
    text: &str,
    kind: ManagedFileKind,
    ranges: &mut Vec<ManagedRange>,
    active: &mut Option<(usize, String)>,
) -> Result<(), MaterialsError> {
    let Some((start, mut key)) = active.take() else {
        return Err(ambiguous());
    };
    if is_license_kind(kind) {
        key = license_scope_key(&scan, start)?;
    } else if !scope_keys_match(&key, text) {
        return Err(ambiguous());
    }
    ranges.push(ManagedRange {
        start,
        end: scan.offset + scan.line.len(),
        key,
    });
    Ok(())
}

fn scope_keys_match(key: &str, text: &str) -> bool {
    let end_key = text
        .trim_start_matches("<!-- END AHCL KIT MANAGED SCOPE: ")
        .trim_end_matches(" -->");
    key == end_key
}

fn license_scope_key(scan: &ScopeScan<'_>, start: usize) -> Result<String, MaterialsError> {
    let body = scan
        .content
        .get(start..scan.offset + scan.line.len())
        .ok_or_else(ambiguous)?;
    let marker = "<!-- AHCL KIT MANAGED SCOPE:";
    if body.matches(marker).count() > 1 {
        return Err(ambiguous());
    }
    let Some(marker_start) = body.find(marker) else {
        return Ok(String::new());
    };
    Ok(marker_key(&body[marker_start..]))
}

fn marker_key(tail: &str) -> String {
    let marker_line = tail.lines().next().unwrap_or_default();
    marker_line
        .trim_start_matches("<!-- AHCL KIT MANAGED SCOPE:")
        .trim()
        .trim_end_matches("-->")
        .trim()
        .to_owned()
}

fn ambiguous() -> MaterialsError {
    MaterialsError::new(MaterialsErrorCode::AmbiguousManagedContent)
}
