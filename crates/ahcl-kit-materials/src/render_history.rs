// crates/ahcl-kit-materials/src/render_history.rs - AHCL licensing history rendering.
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

use crate::render;
use ahcl_kit_config::{EffectiveConfig, HistoryKind, HistoryRecord};
use ahcl_kit_core::UtcDate;

#[path = "history_text.rs"]
pub(crate) mod text;

pub(crate) const CONFIGURED: &str = "<!-- AHCL KIT HISTORY CONFIGURED -->";
pub(crate) const SUMMARY_BEGIN: &str = "<!-- BEGIN AHCL KIT HISTORY SUMMARY -->";
pub(crate) const SUMMARY_END: &str = "<!-- END AHCL KIT HISTORY SUMMARY -->";
pub(crate) const RECORD_BEGIN: &str = "<!-- BEGIN AHCL KIT HISTORY: ";
pub(crate) const RECORD_END: &str = "<!-- END AHCL KIT HISTORY: ";

pub(crate) fn version_adoption(
    config: &EffectiveConfig,
    initial: UtcDate,
    segment_start: UtcDate,
) -> String {
    let history = config.license().history();
    let mut body = String::new();
    if history.is_explicit() {
        body.push_str(CONFIGURED);
        body.push('\n');
    }
    body.push_str(&summary(config, initial, segment_start));
    for record in history.records() {
        body.push('\n');
        body.push_str(&render_record(record));
    }
    format!(
        "# AHCL Version Adoption\n\n{}",
        render::managed_block(config, &body)
    )
}

fn summary(config: &EffectiveConfig, initial: UtcDate, segment: UtcDate) -> String {
    let history = config.license().history();
    let initial = config.project().adoption_date().unwrap_or(initial);
    let mut body = format!("{SUMMARY_BEGIN}\n{}\n", text::MARKER);
    push_field(&mut body, "covered-scope", scope(config));
    body.push_str(&format!(
        "- Initial AHCL adoption date: {initial}\n\
         - Current Continuous AHCL Licensing Segment: {}\n\
         - Current revision lineage: {}\n\
         - Continuous AHCL Licensing Segment beginning: {segment}\n\
         - Applicable version: AHCL {}\n",
        text::literal(history.current_segment()),
        text::literal(history.current_lineage()),
        config.license().version().as_str(),
    ));
    if let Some(date) = history.effective_date() {
        body.push_str(&format!("- Current version Effective Date: {date}\n"));
    }
    if interrupted(config) {
        body.push_str("- AHCL licensing status: interrupted; the version above describes the closed segment.\n");
    }
    body.push_str("\n## Version Adoption Events\n\n");
    if !history
        .records()
        .iter()
        .any(|record| record.kind() == HistoryKind::VersionAdoption)
    {
        body.push_str("None.\n");
    }
    body.push_str(SUMMARY_END);
    body.push('\n');
    body
}

pub(crate) fn interrupted(config: &EffectiveConfig) -> bool {
    let history = config.license().history();
    history.records().iter().any(|record| {
        record.kind() == HistoryKind::Interruption
            && record.segment() == history.current_segment()
            && record.lineage() == history.current_lineage()
            && record.covered_scope() == scope(config)
    })
}

pub(crate) fn scope(config: &EffectiveConfig) -> &str {
    ahcl_kit_config::matching_scope(config.license().covered_scope())
}

fn render_record(record: &HistoryRecord) -> String {
    let mut body = format!(
        "{RECORD_BEGIN}{} -->\n{}\n### {}: {}\n\n",
        record.id(),
        text::MARKER,
        record.kind().as_str(),
        text::literal(record.id()),
    );
    for (key, value) in record.fields() {
        push_field(&mut body, key, value);
    }
    body.push_str(&format!("{RECORD_END}{} -->\n", record.id()));
    body
}

fn push_field(body: &mut String, key: &str, value: &str) {
    let label = field_label(key);
    let literal = text::literal(value);
    let mut lines = literal.split('\n');
    body.push_str(&format!(
        "- {label}: {}\n",
        lines.next().unwrap_or_default()
    ));
    for line in lines {
        body.push_str("  ");
        body.push_str(line);
        body.push('\n');
    }
}

fn field_label(key: &str) -> String {
    match key {
        "effective-date" => "Effective Date".to_owned(),
        "version" => "New AHCL version".to_owned(),
        "from-version" => "Former AHCL version".to_owned(),
        "authority" => "Authority".to_owned(),
        "signature" => "Signature".to_owned(),
        _ => title_label(key),
    }
}

fn title_label(key: &str) -> String {
    let mut characters = key.chars();
    let Some(first) = characters.next() else {
        return String::new();
    };
    format!(
        "{}{}",
        first.to_ascii_uppercase(),
        characters.collect::<String>().replace('-', " ")
    )
}
