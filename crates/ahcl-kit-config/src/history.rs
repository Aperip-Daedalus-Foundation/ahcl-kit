// crates/ahcl-kit-config/src/history.rs - Explicit licensing history and component overrides.
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

use crate::{
    AhclVersion, ConfigDocument, ConfigError, ConfigValue, LicenseSettings, ProjectSettings,
};
use ahcl_kit_core::UtcDate;
use std::collections::BTreeMap;

#[path = "history_parse.rs"]
mod parse;
#[path = "history_validate.rs"]
mod validate;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryKind {
    InitialAdoption,
    VersionAdoption,
    Interruption,
    Readoption,
    Record,
}

impl HistoryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InitialAdoption => "initial-adoption",
            Self::VersionAdoption => "version-adoption",
            Self::Interruption => "interruption",
            Self::Readoption => "readoption",
            Self::Record => "record",
        }
    }

    pub(super) fn starts_segment(self) -> bool {
        matches!(self, Self::InitialAdoption | Self::Readoption)
    }

    pub(super) fn adopts_version(self) -> bool {
        matches!(
            self,
            Self::InitialAdoption | Self::Readoption | Self::VersionAdoption
        )
    }
}

/// A supplied record snapshot. Its strings are retained exactly as parsed.
/// Validation checks configuration consistency, not signatures or legal authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryRecord {
    fields: BTreeMap<String, String>,
    kind: HistoryKind,
    effective_date: Option<UtcDate>,
    version: Option<AhclVersion>,
    from_version: Option<AhclVersion>,
}

impl HistoryRecord {
    pub fn id(&self) -> &str {
        &self.fields["id"]
    }

    pub fn kind(&self) -> HistoryKind {
        self.kind
    }

    pub fn segment(&self) -> &str {
        &self.fields["segment"]
    }

    /// The supplied lineage, or `main` when the field is absent.
    pub fn lineage(&self) -> &str {
        self.fields
            .get("lineage")
            .map(String::as_str)
            .unwrap_or("main")
    }

    pub fn effective_date(&self) -> Option<UtcDate> {
        self.effective_date
    }

    pub fn version(&self) -> Option<AhclVersion> {
        self.version
    }

    pub fn from_version(&self) -> Option<AhclVersion> {
        self.from_version
    }

    pub fn covered_scope(&self) -> &str {
        &self.fields["covered-scope"]
    }

    pub fn fields(&self) -> &BTreeMap<String, String> {
        &self.fields
    }

    fn resolved_value(&self) -> ConfigValue {
        ConfigValue::Object(
            self.fields
                .iter()
                .map(|(key, value)| (key.clone(), ConfigValue::String(value.clone())))
                .collect(),
        )
    }
}

/// Optional local history settings, separate from `ComponentIdentity` for compatibility.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HistoryOverrides {
    current_segment: Option<String>,
    current_lineage: Option<String>,
    segment_start: Option<UtcDate>,
    effective_date: Option<UtcDate>,
    records: Option<Vec<HistoryRecord>>,
}

impl HistoryOverrides {
    /// Returns `None` when the section supplies no history settings.
    /// An explicit `history = []` returns an override with an empty record list.
    pub fn parse(document: &ConfigDocument, section: &str) -> Result<Option<Self>, ConfigError> {
        parse::overrides(document, section)
    }

    pub fn resolved_value(&self) -> ConfigValue {
        ConfigValue::Object(vec![
            (
                "current_segment".to_owned(),
                string_value(self.current_segment.as_deref()),
            ),
            (
                "current_lineage".to_owned(),
                string_value(self.current_lineage.as_deref()),
            ),
            ("segment_start".to_owned(), date_value(self.segment_start)),
            ("effective_date".to_owned(), date_value(self.effective_date)),
            (
                "records".to_owned(),
                self.records
                    .as_ref()
                    .map(|records| {
                        ConfigValue::Array(
                            records.iter().map(HistoryRecord::resolved_value).collect(),
                        )
                    })
                    .unwrap_or(ConfigValue::Null),
            ),
            (
                "explicit".to_owned(),
                ConfigValue::Bool(self.records.is_some()),
            ),
            (
                "current_lineage_explicit".to_owned(),
                ConfigValue::Bool(self.current_lineage.is_some()),
            ),
        ])
    }

    fn overlay(&mut self, local: &Self) {
        self.overlay_segment(local);
        self.overlay_lineage(local);
        if local.segment_start.is_some() {
            self.segment_start = local.segment_start;
        }
        if local.effective_date.is_some() {
            self.effective_date = local.effective_date;
        }
        if let Some(records) = &local.records {
            self.records = Some(records.clone());
        }
    }

    fn overlay_segment(&mut self, local: &Self) {
        if let Some(value) = &local.current_segment {
            if self.current_segment.as_deref().unwrap_or("initial") != value {
                self.segment_start = None;
                self.effective_date = None;
            }
            self.current_segment = Some(value.clone());
        }
    }

    fn overlay_lineage(&mut self, local: &Self) {
        if let Some(value) = &local.current_lineage {
            if self.current_lineage.as_deref().unwrap_or("main") != value {
                self.segment_start = None;
                self.effective_date = None;
            }
            self.current_lineage = Some(value.clone());
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LicensingHistory {
    supplied: HistoryOverrides,
    current_segment: String,
    current_lineage: String,
    segment_start: Option<UtcDate>,
    effective_date: Option<UtcDate>,
}

impl Default for LicensingHistory {
    fn default() -> Self {
        Self::initial(None)
    }
}

impl LicensingHistory {
    pub fn current_segment(&self) -> &str {
        &self.current_segment
    }

    pub fn current_lineage(&self) -> &str {
        &self.current_lineage
    }

    pub fn segment_start(&self) -> Option<UtcDate> {
        self.segment_start
    }

    pub fn effective_date(&self) -> Option<UtcDate> {
        self.effective_date
    }

    pub fn records(&self) -> &[HistoryRecord] {
        self.supplied.records.as_deref().unwrap_or_default()
    }

    /// Whether a history list was supplied, including an explicit empty list.
    pub fn is_explicit(&self) -> bool {
        self.supplied.records.is_some()
    }

    pub fn resolved_value(&self) -> ConfigValue {
        ConfigValue::Object(vec![
            (
                "current_segment".to_owned(),
                ConfigValue::String(self.current_segment.clone()),
            ),
            (
                "current_lineage".to_owned(),
                ConfigValue::String(self.current_lineage.clone()),
            ),
            ("segment_start".to_owned(), date_value(self.segment_start)),
            ("effective_date".to_owned(), date_value(self.effective_date)),
            (
                "records".to_owned(),
                ConfigValue::Array(
                    self.records()
                        .iter()
                        .map(HistoryRecord::resolved_value)
                        .collect(),
                ),
            ),
            ("explicit".to_owned(), ConfigValue::Bool(self.is_explicit())),
            (
                "current_segment_explicit".to_owned(),
                ConfigValue::Bool(self.supplied.current_segment.is_some()),
            ),
            (
                "current_lineage_explicit".to_owned(),
                ConfigValue::Bool(self.supplied.current_lineage.is_some()),
            ),
            (
                "segment_start_explicit".to_owned(),
                ConfigValue::Bool(self.supplied.segment_start.is_some()),
            ),
            (
                "effective_date_explicit".to_owned(),
                ConfigValue::Bool(self.supplied.effective_date.is_some()),
            ),
        ])
    }

    pub(crate) fn initial(adoption_date: Option<UtcDate>) -> Self {
        Self {
            supplied: HistoryOverrides::default(),
            current_segment: "initial".to_owned(),
            current_lineage: "main".to_owned(),
            segment_start: adoption_date,
            effective_date: None,
        }
    }

    pub(crate) fn without_scalar_dates(&self) -> Self {
        let mut cloned = self.clone();
        cloned.supplied.segment_start = None;
        cloned.supplied.effective_date = None;
        cloned.segment_start = None;
        cloned.effective_date = None;
        cloned
    }

    pub(crate) fn resolve(
        supplied: HistoryOverrides,
        license: &LicenseSettings,
        project: &ProjectSettings,
        path: &str,
    ) -> Result<Self, ConfigError> {
        validate::resolve(supplied, license, project, path)
    }

    pub(crate) fn with_overrides(
        &self,
        local: Option<&HistoryOverrides>,
        license: &LicenseSettings,
        project: &ProjectSettings,
        path: &str,
    ) -> Result<Self, ConfigError> {
        let mut supplied = self.supplied.clone();
        if let Some(local) = local {
            supplied.overlay(local);
        }
        Self::resolve(supplied, license, project, path)
    }
}

fn date_value(value: Option<UtcDate>) -> ConfigValue {
    value
        .map(|date| ConfigValue::String(date.to_string()))
        .unwrap_or(ConfigValue::Null)
}

fn string_value(value: Option<&str>) -> ConfigValue {
    value
        .map(|value| ConfigValue::String(value.to_owned()))
        .unwrap_or(ConfigValue::Null)
}

pub(crate) fn matching_scope(scope: &str) -> &str {
    if scope.trim().is_empty() {
        "the project as a whole"
    } else {
        scope
    }
}

fn invalid<T>(path: &str, message: &str) -> Result<T, ConfigError> {
    Err(ConfigError::InvalidValue {
        path: path.to_owned(),
        message: message.to_owned(),
    })
}
