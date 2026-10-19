// crates/ahcl-kit-materials/src/project_history.rs - Preserve AHCL historical records.
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

use crate::{LayoutPolicy, MaterialsError, render, render_history};
use ahcl_kit_config::{AhclVersion, EffectiveConfig, HistoryKind, HistoryRecord};
use ahcl_kit_core::{ProjectEntry, ProjectView, UtcDate};
use std::collections::BTreeMap;
use std::ops::Range;

pub(crate) fn conflict() -> MaterialsError {
    MaterialsError::filesystem(
        "materials.history_conflict",
        "AHCL history conflicts with configuration; preserve the existing record and configure an explicit licensing event",
    )
}

pub(crate) fn notice(
    view: &dyn ProjectView,
    config: &EffectiveConfig,
    layout: &LayoutPolicy,
    initial: UtcDate,
) -> Result<String, MaterialsError> {
    let body = existing_notice(view, config, layout)?;
    validate_notice_version(config, &body)?;
    let start = notice_start(config, &body, initial)?;
    Ok(render::project_notice(config, layout, start))
}

pub(crate) fn adoption(
    view: &dyn ProjectView,
    config: &EffectiveConfig,
    layout: &LayoutPolicy,
    initial: UtcDate,
) -> Result<String, MaterialsError> {
    let body = existing_notice(view, config, layout)?;
    let start = notice_start(config, &body, initial)?;
    Ok(render::version_adoption(config, initial, start))
}

fn existing_notice(
    view: &dyn ProjectView,
    config: &EffectiveConfig,
    layout: &LayoutPolicy,
) -> Result<String, MaterialsError> {
    let entry = view.entry(&layout.project_notice_path()?).map_err(|_| {
        MaterialsError::filesystem("materials.view", "project notice could not be read")
    })?;
    match entry {
        ProjectEntry::Absent => Ok(String::new()),
        ProjectEntry::Other => Err(conflict()),
        ProjectEntry::File(bytes) => {
            let text = String::from_utf8(bytes).map_err(|_| conflict())?;
            scope_body(&text, &render::scope_key(config))
        }
    }
}

fn scope_body(text: &str, key: &str) -> Result<String, MaterialsError> {
    let begin = format!("<!-- BEGIN AHCL KIT MANAGED SCOPE: {key} -->");
    let end = format!("<!-- END AHCL KIT MANAGED SCOPE: {key} -->");
    let Some(range) = unique_range(text, &begin, &end)? else {
        return Ok(if text.contains("<!-- BEGIN AHCL KIT MANAGED SCOPE:") {
            String::new()
        } else {
            text.to_owned()
        });
    };
    Ok(text[range].to_owned())
}

fn notice_start(
    config: &EffectiveConfig,
    old: &str,
    initial: UtcDate,
) -> Result<UtcDate, MaterialsError> {
    let history = config.license().history();
    let prior = prior_start(old)?;
    if let Some(start) = configured_start(config) {
        let same_group = notice_group(old)
            == (
                history.current_segment().to_owned(),
                history.current_lineage().to_owned(),
            );
        if same_group && prior.is_some_and(|prior| prior != start) {
            return Err(conflict());
        }
        return Ok(start);
    }
    Ok(prior.or(history.segment_start()).unwrap_or(initial))
}

fn prior_start(old: &str) -> Result<Option<UtcDate>, MaterialsError> {
    let dates: Vec<_> = old
        .lines()
        .filter_map(|line| line.strip_prefix("- Continuous AHCL Licensing Segment beginning: "))
        .collect();
    match dates.as_slice() {
        [] => Ok(None),
        [date] => ahcl_kit_config::parse_date_at(date.trim(), "project notice segment beginning")
            .map(Some)
            .map_err(|_| conflict()),
        _ => Err(conflict()),
    }
}

fn configured_start(config: &EffectiveConfig) -> Option<UtcDate> {
    if config.license().history().segment_start_explicit() {
        return config.license().history().segment_start();
    }
    config
        .license()
        .history()
        .records()
        .iter()
        .find(|record| record_starts_current_segment(config, record))
        .and_then(HistoryRecord::effective_date)
}

fn record_starts_current_segment(config: &EffectiveConfig, record: &HistoryRecord) -> bool {
    matches_current_group(config, record)
        && matches!(
            record.kind(),
            HistoryKind::InitialAdoption | HistoryKind::Readoption
        )
}

fn validate_notice_version(config: &EffectiveConfig, old: &str) -> Result<(), MaterialsError> {
    let Some(prior) = prior_version(old)? else {
        return Ok(());
    };
    if prior == config.license().version() {
        return Ok(());
    }
    let has_event = config
        .license()
        .history()
        .records()
        .iter()
        .any(|record| authorizes_version(config, old, prior, record));
    if has_event { Ok(()) } else { Err(conflict()) }
}

fn prior_version(old: &str) -> Result<Option<AhclVersion>, MaterialsError> {
    let versions: Vec<_> = old
        .lines()
        .filter_map(|line| line.strip_prefix("- Applicable version: AHCL "))
        .collect();
    match versions.as_slice() {
        [] => Ok(None),
        [value] => ahcl_kit_config::parse_version(
            value.split_whitespace().next().unwrap_or_default(),
            "project notice version",
        )
        .map(Some)
        .map_err(|_| conflict()),
        _ => Err(conflict()),
    }
}

fn authorizes_version(
    config: &EffectiveConfig,
    old: &str,
    prior: AhclVersion,
    record: &HistoryRecord,
) -> bool {
    if !matches_current_record(config, record) {
        return false;
    }
    match record.kind() {
        HistoryKind::VersionAdoption => authorizes_upgrade(config, old, prior, record),
        HistoryKind::Readoption => previous_matches(old, record) && distinct_segment(old, record),
        _ => false,
    }
}

fn authorizes_upgrade(
    config: &EffectiveConfig,
    old: &str,
    prior: AhclVersion,
    record: &HistoryRecord,
) -> bool {
    config.license().version() > prior
        && matches_notice_group(old, record.segment(), record.lineage())
        && record
            .from_version()
            .is_some_and(|base| reachable_version(config, prior, base))
}

fn reachable_version(config: &EffectiveConfig, prior: AhclVersion, target: AhclVersion) -> bool {
    // A lower parallel baseline does not revert the already-applicable maximum.
    // Higher baselines require supplied upgrade steps; every step strictly
    // increases its version, so this traversal cannot cycle or infer dates.
    if target <= prior {
        return true;
    }
    config.license().history().records().iter().any(|record| {
        upgrade_step(config, record, target)
            && record
                .from_version()
                .is_some_and(|base| reachable_version(config, prior, base))
    })
}

fn upgrade_step(config: &EffectiveConfig, record: &HistoryRecord, target: AhclVersion) -> bool {
    record.kind() == HistoryKind::VersionAdoption
        && matches_current_group(config, record)
        && record.version() == Some(target)
}

fn previous_matches(old: &str, record: &HistoryRecord) -> bool {
    let Some(previous) = record.fields().get("previous-segment") else {
        return true;
    };
    let lineage = record
        .fields()
        .get("previous-lineage")
        .map(String::as_str)
        .unwrap_or_else(|| record.lineage());
    matches_notice_group(old, previous, lineage)
}

fn matches_notice_group(old: &str, segment: &str, lineage: &str) -> bool {
    summary_value(old, "Current Continuous AHCL Licensing Segment")
        .is_none_or(|value| value.trim() == segment)
        && summary_value(old, "Current revision lineage")
            .is_none_or(|value| value.trim() == lineage)
}

fn notice_group(text: &str) -> (String, String) {
    (
        summary_value(text, "Current Continuous AHCL Licensing Segment")
            .unwrap_or_else(|| "initial".to_owned()),
        summary_value(text, "Current revision lineage").unwrap_or_else(|| "main".to_owned()),
    )
}

fn matches_current_record(config: &EffectiveConfig, record: &HistoryRecord) -> bool {
    matches_current_group(config, record) && record.version() == Some(config.license().version())
}

fn matches_current_group(config: &EffectiveConfig, record: &HistoryRecord) -> bool {
    let history = config.license().history();
    record.segment() == history.current_segment()
        && record.lineage() == history.current_lineage()
        && record.covered_scope() == crate::render_history::scope(config)
}

fn distinct_segment(old: &str, record: &HistoryRecord) -> bool {
    let previous = summary_value(old, "Current Continuous AHCL Licensing Segment")
        .or_else(|| record.fields().get("previous-segment").cloned())
        .unwrap_or_else(|| "initial".to_owned());
    let lineage = summary_value(old, "Current revision lineage")
        .or_else(|| record.fields().get("previous-lineage").cloned())
        .unwrap_or_else(|| "main".to_owned());
    (record.segment(), record.lineage()) != (previous.trim(), lineage.trim())
}

pub(crate) fn merge_adoption(old: &str, desired: &str) -> Result<String, MaterialsError> {
    let old_records = record_ranges(old)?;
    unique_range(
        old,
        render_history::SUMMARY_BEGIN,
        render_history::SUMMARY_END,
    )?;
    if !desired.contains(render_history::CONFIGURED) {
        return Ok(old.to_owned());
    }
    let mut output = old.to_owned();
    let desired_records = record_ranges(desired)?;
    for (id, range) in &desired_records {
        append_record(&mut output, old, &desired[range.clone()], id, &old_records)?;
    }
    merge_summary(&output, desired)
}

pub(crate) fn validate_unscoped_records(old: &str, desired: &str) -> Result<(), MaterialsError> {
    let old_records = record_ranges(old)?;
    for (id, range) in record_ranges(desired)? {
        let Some(prior) = old_records.get(&id) else {
            continue;
        };
        let original = &old[prior.clone()];
        let record = &desired[range];
        if record_scope(original)? == record_scope(record)? {
            compatible_fields(original, record)?;
        }
    }
    Ok(())
}
fn record_scope(record: &str) -> Result<String, MaterialsError> {
    let scope = fields(record)?
        .into_iter()
        .find(|field| field.key == "Covered scope")
        .ok_or_else(conflict)?;
    Ok(render_history::text::normalized(
        record,
        record[scope.range].trim_end_matches(['\r', '\n']),
    ))
}

fn record_ranges(text: &str) -> Result<BTreeMap<String, Range<usize>>, MaterialsError> {
    let mut records = BTreeMap::new();
    let mut active = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        observe_record(line, offset, &mut active, &mut records)?;
        offset += line.len();
    }
    if active.is_some() {
        return Err(conflict());
    }
    Ok(records)
}

fn observe_record(
    line: &str,
    offset: usize,
    active: &mut Option<(String, usize)>,
    records: &mut BTreeMap<String, Range<usize>>,
) -> Result<(), MaterialsError> {
    let line_text = line.trim_end_matches(['\r', '\n']);
    if let Some(id) = marker_id(line_text, render_history::RECORD_BEGIN) {
        if active.is_some() {
            return Err(conflict());
        }
        *active = Some((id.to_owned(), offset));
    } else if let Some(id) = marker_id(line_text, render_history::RECORD_END) {
        finish_record(id, offset + line.len(), active, records)?;
    }
    Ok(())
}

fn finish_record(
    id: &str,
    end: usize,
    active: &mut Option<(String, usize)>,
    records: &mut BTreeMap<String, Range<usize>>,
) -> Result<(), MaterialsError> {
    let (key, start) = active.take().ok_or_else(conflict)?;
    if key != id {
        return Err(conflict());
    }
    if records.insert(key, start..end).is_some() {
        return Err(conflict());
    }
    Ok(())
}

fn marker_id<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    line.strip_prefix(prefix)?.strip_suffix(" -->")
}

fn append_record(
    output: &mut String,
    old: &str,
    record: &str,
    id: &str,
    old_records: &BTreeMap<String, Range<usize>>,
) -> Result<(), MaterialsError> {
    if let Some(range) = old_records.get(id) {
        return compatible_fields(&old[range.clone()], record);
    }
    insert_at_end(output, &format!("\n{record}"));
    Ok(())
}

fn merge_summary(old: &str, desired: &str) -> Result<String, MaterialsError> {
    let summary = unique_range(
        desired,
        render_history::SUMMARY_BEGIN,
        render_history::SUMMARY_END,
    )?
    .ok_or_else(conflict)?;
    let mut replacement = desired[summary].to_owned();
    if old.contains("- Version adoption event:") || old.contains("- Kind: version-adoption") {
        replacement = replacement.replace(
            "\nNone.\n",
            "\nSee the preserved records in this document.\n",
        );
    }
    let mut output = old.to_owned();
    match unique_range(
        old,
        render_history::SUMMARY_BEGIN,
        render_history::SUMMARY_END,
    )? {
        Some(range) => {
            let replacement = preserve_summary_date(&old[range.clone()], &replacement)?;
            if old[range.clone()].replace("\r\n", "\n") != replacement.replace("\r\n", "\n") {
                output.replace_range(range, &replacement);
            }
        }
        None => {
            insert_at_end(&mut output, &format!("\n{replacement}\n"));
        }
    }
    Ok(output)
}

fn preserve_summary_date(old: &str, desired: &str) -> Result<String, MaterialsError> {
    if !same_summary_state(old, desired)
        || summary_value(desired, "Current version Effective Date").is_some()
    {
        return Ok(desired.to_owned());
    }
    let date = fields(old)?
        .into_iter()
        .find(|field| field.key == "Current version Effective Date");
    let mut output = desired.to_owned();
    if let Some(date) = date {
        let position = desired
            .find("\n## Version Adoption Events")
            .ok_or_else(conflict)?;
        output.insert_str(position, &old[date.range]);
    }
    Ok(output)
}

fn same_summary_state(old: &str, desired: &str) -> bool {
    summary_value(old, "Applicable version") == summary_value(desired, "Applicable version")
        && notice_group(old) == notice_group(desired)
}

fn summary_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (label, value) = line.strip_prefix("- ")?.split_once(": ")?;
        (label == key).then(|| render_history::text::normalized(text, value))
    })
}

fn unique_range(
    text: &str,
    begin: &str,
    end: &str,
) -> Result<Option<Range<usize>>, MaterialsError> {
    let starts = line_offsets(text, begin);
    let ends = line_offsets(text, end);
    match (starts.as_slice(), ends.as_slice()) {
        ([], []) => Ok(None),
        ([start], [finish]) if start < finish => Ok(Some(*start..finish + end.len())),
        _ => Err(conflict()),
    }
}

fn line_offsets(text: &str, marker: &str) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == marker {
            offsets.push(offset);
        }
        offset += line.len();
    }
    offsets
}

fn insert_at_end(output: &mut String, addition: &str) {
    let position = output
        .rfind("<!-- END AHCL KIT MANAGED SCOPE:")
        .unwrap_or(output.len());
    let separator = if position > 0 && !output[..position].ends_with('\n') {
        "\n"
    } else {
        ""
    };
    output.insert_str(position, &format!("{separator}{addition}"));
}

struct Field<'a> {
    key: &'a str,
    range: Range<usize>,
}

fn fields(text: &str) -> Result<Vec<Field<'_>>, MaterialsError> {
    let mut fields: Vec<Field<'_>> = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if let Some(key) = field_key(line) {
            if fields.iter().any(|field| field.key == key) {
                return Err(conflict());
            }
            fields.push(Field {
                key,
                range: offset..offset + line.len(),
            });
        } else if line.starts_with("  ") {
            if let Some(field) = fields.last_mut().filter(|field| field.range.end == offset) {
                field.range.end += line.len();
            }
        }
        offset += line.len();
    }
    Ok(fields)
}

fn field_key(line: &str) -> Option<&str> {
    line.strip_prefix("- ")?.split_once(':').map(|(key, _)| key)
}

fn compatible_fields(old: &str, desired: &str) -> Result<(), MaterialsError> {
    let old_fields = fields(old)?;
    for field in fields(desired)? {
        let prior = old_fields
            .iter()
            .find(|prior| prior.key == field.key)
            .ok_or_else(conflict)?;
        let old_value = render_history::text::normalized(old, &old[prior.range.clone()]);
        if old_value != render_history::text::normalized(desired, &desired[field.range]) {
            return Err(conflict());
        }
    }
    Ok(())
}

pub(crate) fn merge_notice(old: &str, desired: &str) -> Result<String, MaterialsError> {
    let mut output = old.to_owned();
    if !same_summary_state(old, desired) {
        remove_missing_notice_field(&mut output, desired, "Current version Effective Date")?;
        remove_missing_notice_field(&mut output, desired, "Effective Date")?;
    }
    for field in fields(desired)?.into_iter().rev() {
        update_notice_field(&mut output, desired, &field)?;
    }
    Ok(output)
}

fn remove_missing_notice_field(
    output: &mut String,
    desired: &str,
    key: &str,
) -> Result<(), MaterialsError> {
    if summary_value(desired, key).is_some() {
        return Ok(());
    }
    if let Some(field) = fields(output)?.iter().find(|field| field.key == key) {
        output.replace_range(notice_field_range(output, field), "");
    }
    Ok(())
}

fn update_notice_field(
    output: &mut String,
    desired: &str,
    field: &Field<'_>,
) -> Result<(), MaterialsError> {
    let current = fields(output)?;
    let prior = current.iter().find(|prior| prior.key == field.key);
    let replacement = &desired[field.range.clone()];
    if let Some(prior) = prior {
        let range = notice_field_range(output, prior);
        let original = output[range.clone()].to_owned();
        if original.replace("\r\n", "\n") == replacement.replace("\r\n", "\n") {
            return Ok(());
        }
        preserve_origin(output, field.key, &original, replacement);
        output.replace_range(range, replacement);
    } else {
        insert_at_end(output, replacement);
    }
    Ok(())
}

fn notice_field_range(text: &str, field: &Field<'_>) -> Range<usize> {
    let mut lines = text[field.range.clone()].split_inclusive('\n');
    let first = lines.next().unwrap_or_default().len();
    let continuation = if field.key == "Right holders" {
        lines
            .take_while(|line| line.starts_with("  - "))
            .map(str::len)
            .sum::<usize>()
    } else {
        0
    };
    field.range.start..field.range.start + first + continuation
}

fn preserve_origin(output: &mut String, key: &str, old: &str, desired: &str) {
    if key != "Canonical repository" || old == desired {
        return;
    }
    if output.contains("- Original canonical repository:") {
        return;
    }
    insert_at_end(
        output,
        &old.replacen("Canonical repository:", "Original canonical repository:", 1),
    );
}
