// crates/ahcl-kit-config/src/history_validate.rs - Scope- and lineage-local history consistency.
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

use super::{
    HistoryKind, HistoryOverrides, HistoryRecord, LicensingHistory, invalid, matching_scope,
};
use crate::{AhclVersion, ConfigError, LicenseSettings, ProjectSettings};
use ahcl_kit_core::UtcDate;
use std::collections::{BTreeMap, BTreeSet};

type SegmentKey<'a> = (&'a str, &'a str, &'a str);
type Segments<'a> = BTreeMap<SegmentKey<'a>, Segment<'a>>;

#[derive(Default)]
struct Segment<'a> {
    start: Option<&'a HistoryRecord>,
    interruption: Option<&'a HistoryRecord>,
    events: Vec<&'a HistoryRecord>,
}

impl<'a> Segment<'a> {
    fn push(&mut self, record: &'a HistoryRecord, path: &str) -> Result<(), ConfigError> {
        if record.kind().starts_segment() {
            remember_boundary(&mut self.start, record, path, "segment start")?;
        }
        if record.kind() == HistoryKind::Interruption {
            remember_boundary(&mut self.interruption, record, path, "interruption")?;
        }
        self.events.push(record);
        Ok(())
    }
}

pub(super) fn resolve(
    supplied: HistoryOverrides,
    license: &LicenseSettings,
    project: &ProjectSettings,
    path: &str,
) -> Result<LicensingHistory, ConfigError> {
    let current_segment = supplied
        .current_segment
        .clone()
        .unwrap_or_else(|| "initial".to_owned());
    let current_lineage = supplied
        .current_lineage
        .clone()
        .unwrap_or_else(|| "main".to_owned());
    let segments = collect_segments(supplied.records.as_deref().unwrap_or_default(), path)?;
    let current_key = (
        current_segment.as_str(),
        matching_scope(license.covered_scope()),
        current_lineage.as_str(),
    );
    let current = segments.get(&current_key);
    let segment_start = resolve_start(&supplied, &current_segment, current, project, path)?;
    validate_segments(&segments, current_key, segment_start, path)?;
    validate_readoptions(&segments, path)?;
    validate_active_segment(current, license.enabled(), path)?;
    let effective_date =
        resolve_effective(supplied.effective_date, current, license.version(), path)?;
    validate_current_dates(
        segment_start,
        effective_date,
        project.adoption_date(),
        current,
        path,
    )?;
    Ok(LicensingHistory {
        supplied,
        current_segment,
        current_lineage,
        segment_start,
        effective_date,
    })
}

fn collect_segments<'a>(
    records: &'a [HistoryRecord],
    path: &str,
) -> Result<Segments<'a>, ConfigError> {
    let mut segments = Segments::new();
    let mut ids = BTreeSet::new();
    for record in records {
        if !ids.insert(record.id()) {
            return invalid(&record_path(path, record), "duplicate history record id");
        }
        segments
            .entry((record.segment(), record.covered_scope(), record.lineage()))
            .or_default()
            .push(record, path)?;
    }
    Ok(segments)
}

fn remember_boundary<'a>(
    slot: &mut Option<&'a HistoryRecord>,
    record: &'a HistoryRecord,
    path: &str,
    label: &str,
) -> Result<(), ConfigError> {
    if slot.replace(record).is_some() {
        return invalid(
            &record_path(path, record),
            &format!(
                "duplicate or conflicting {label} for the same segment, covered-scope, and lineage"
            ),
        );
    }
    Ok(())
}

fn resolve_start(
    supplied: &HistoryOverrides,
    current_segment: &str,
    current: Option<&Segment<'_>>,
    project: &ProjectSettings,
    path: &str,
) -> Result<Option<UtcDate>, ConfigError> {
    let recorded = current
        .and_then(|segment| segment.start)
        .and_then(HistoryRecord::effective_date);
    if let (Some(explicit), Some(recorded)) = (supplied.segment_start, recorded) {
        if explicit != recorded {
            return invalid(
                &format!("{path}.segment-start"),
                "conflicts with the matching segment start record",
            );
        }
    }
    let fallback = (current_segment == "initial")
        .then_some(project.adoption_date())
        .flatten();
    let start = recorded.or(supplied.segment_start).or(fallback);
    if current_segment != "initial" && start.is_none() {
        return invalid(
            &format!("{path}.segment-start"),
            "a non-initial current segment requires segment-start or a matching initial-adoption/readoption record",
        );
    }
    Ok(start)
}

fn validate_segments(
    segments: &Segments<'_>,
    current_key: SegmentKey<'_>,
    current_start: Option<UtcDate>,
    path: &str,
) -> Result<(), ConfigError> {
    for (key, segment) in segments {
        let recorded = segment.start.and_then(HistoryRecord::effective_date);
        let fallback = (*key == current_key).then_some(current_start).flatten();
        let start = recorded.or(fallback);
        let end = segment.interruption.and_then(HistoryRecord::effective_date);
        for event in &segment.events {
            validate_event_dates(event, start, end, path)?;
        }
    }
    Ok(())
}

fn validate_event_dates(
    event: &HistoryRecord,
    start: Option<UtcDate>,
    end: Option<UtcDate>,
    path: &str,
) -> Result<(), ConfigError> {
    match event.kind() {
        HistoryKind::Record => Ok(()),
        _ => validate_boundary_date(event, start, end, path),
    }
}

fn validate_boundary_date(
    event: &HistoryRecord,
    start: Option<UtcDate>,
    end: Option<UtcDate>,
    path: &str,
) -> Result<(), ConfigError> {
    let date = event
        .effective_date()
        .ok_or_else(|| ConfigError::InvalidValue {
            path: record_path(path, event),
            message: "licensing state events require effective-date".to_owned(),
        })?;
    if start.is_some_and(|start| date < start) {
        return invalid(
            &record_path(path, event),
            "effective-date precedes this segment's start for the same covered-scope and lineage",
        );
    }
    if end.is_some_and(|end| date > end) {
        return invalid(
            &record_path(path, event),
            "effective-date follows this segment's interruption for the same covered-scope and lineage",
        );
    }
    Ok(())
}

fn validate_readoptions(segments: &Segments<'_>, path: &str) -> Result<(), ConfigError> {
    for start in segments
        .values()
        .filter_map(|segment| segment.start)
        .filter(|start| start.kind() == HistoryKind::Readoption)
    {
        validate_previous_segment(segments, start, path)?;
    }
    validate_readoption_cycles(segments, path)
}

fn previous_group(record: &HistoryRecord) -> Option<SegmentKey<'_>> {
    if record.kind() != HistoryKind::Readoption {
        return None;
    }
    let previous = record.fields().get("previous-segment")?;
    let lineage = record
        .fields()
        .get("previous-lineage")
        .map(String::as_str)
        .unwrap_or_else(|| record.lineage());
    Some((previous.as_str(), record.covered_scope(), lineage))
}

fn validate_previous_segment(
    segments: &Segments<'_>,
    start: &HistoryRecord,
    path: &str,
) -> Result<(), ConfigError> {
    let Some(previous) = previous_group(start) else {
        return Ok(());
    };
    if previous == (start.segment(), start.covered_scope(), start.lineage()) {
        return invalid(
            &record_path(path, start),
            "readoption must start a different segment or lineage from its predecessor",
        );
    }
    let interruption = segments
        .get(&previous)
        .and_then(|segment| segment.interruption);
    if !interruption.is_some_and(|record| record.effective_date() <= start.effective_date()) {
        return invalid(
            &record_path(path, start),
            "previous-segment requires an interruption for the same covered-scope and previous-lineage dated no later than readoption",
        );
    }
    Ok(())
}

fn validate_readoption_cycles(segments: &Segments<'_>, path: &str) -> Result<(), ConfigError> {
    let mut complete = BTreeSet::new();
    for key in segments.keys().copied() {
        validate_previous_chain(key, segments, &mut complete, path)?;
    }
    Ok(())
}

fn validate_previous_chain<'a>(
    mut key: SegmentKey<'a>,
    segments: &Segments<'a>,
    complete: &mut BTreeSet<SegmentKey<'a>>,
    path: &str,
) -> Result<(), ConfigError> {
    let mut visiting = BTreeSet::new();
    while !complete.contains(&key) {
        if !visiting.insert(key) {
            return invalid(
                &format!("{path}.history"),
                "readoption previous-segment/previous-lineage links contain a cycle",
            );
        }
        let previous = segments
            .get(&key)
            .and_then(|segment| segment.start)
            .and_then(previous_group);
        let Some(previous) = previous else {
            break;
        };
        key = previous;
    }
    complete.extend(visiting);
    Ok(())
}

fn validate_active_segment(
    current: Option<&Segment<'_>>,
    enabled: bool,
    path: &str,
) -> Result<(), ConfigError> {
    if enabled && current.is_some_and(|segment| segment.interruption.is_some()) {
        return invalid(
            &format!("{path}.enabled"),
            "must be false when the current segment, covered-scope, and lineage have an interruption record",
        );
    }
    Ok(())
}

fn adoption_records<'a>(
    current: Option<&'a Segment<'a>>,
) -> impl Iterator<Item = &'a HistoryRecord> {
    // Each snapshot states its own baseline. List order, effective-time, and
    // supplied evidence do not establish a single version chain or ancestry.
    current
        .into_iter()
        .flat_map(|segment| segment.events.iter().copied())
        .filter(|record| record.kind().adopts_version())
}

fn resolve_effective(
    explicit: Option<UtcDate>,
    current: Option<&Segment<'_>>,
    configured: AhclVersion,
    path: &str,
) -> Result<Option<UtcDate>, ConfigError> {
    let highest = adoption_records(current)
        .filter_map(HistoryRecord::version)
        .max();
    if highest.is_some_and(|version| version > configured) {
        return invalid(
            &format!("{path}.version"),
            "cannot be lower than the highest adopted version in the current segment, covered-scope, and lineage",
        );
    }
    let dates = adoption_records(current)
        .filter(|record| record.version() == Some(configured))
        .filter_map(HistoryRecord::effective_date)
        .collect::<BTreeSet<_>>();
    validate_effective_snapshot(explicit, &dates, path)?;
    let recorded = (dates.len() == 1).then(|| dates.first().copied()).flatten();
    Ok(explicit.or(recorded))
}

fn validate_effective_snapshot(
    explicit: Option<UtcDate>,
    dates: &BTreeSet<UtcDate>,
    path: &str,
) -> Result<(), ConfigError> {
    let Some(date) = explicit else {
        return Ok(());
    };
    if !dates.is_empty() && !dates.contains(&date) {
        return invalid(
            &format!("{path}.effective-date"),
            "conflicts with the matching current highest-version adoption records",
        );
    }
    Ok(())
}

fn validate_current_dates(
    start: Option<UtcDate>,
    effective: Option<UtcDate>,
    adoption: Option<UtcDate>,
    current: Option<&Segment<'_>>,
    path: &str,
) -> Result<(), ConfigError> {
    for (key, date) in [("segment-start", start), ("effective-date", effective)] {
        if let Some(date) = date {
            validate_not_before(
                &format!("{path}.{key}"),
                date,
                adoption,
                "project.adoption-date",
            )?;
        }
    }
    if let Some(date) = effective {
        validate_not_before(
            &format!("{path}.effective-date"),
            date,
            start,
            "segment-start",
        )?;
        validate_effective_end(date, current, path)?;
    }
    Ok(())
}

fn validate_not_before(
    path: &str,
    date: UtcDate,
    earliest: Option<UtcDate>,
    label: &str,
) -> Result<(), ConfigError> {
    if earliest.is_some_and(|earliest| date < earliest) {
        return invalid(path, &format!("cannot precede {label}"));
    }
    Ok(())
}

fn validate_effective_end(
    date: UtcDate,
    current: Option<&Segment<'_>>,
    path: &str,
) -> Result<(), ConfigError> {
    let end = current
        .and_then(|segment| segment.interruption)
        .and_then(HistoryRecord::effective_date);
    if end.is_some_and(|end| date > end) {
        return invalid(
            &format!("{path}.effective-date"),
            "cannot follow the current segment's interruption",
        );
    }
    Ok(())
}

fn record_path(path: &str, record: &HistoryRecord) -> String {
    format!("{path}.history.{}", record.id())
}
