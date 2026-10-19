// crates/ahcl-kit-config/src/history_parse.rs - Strict parsing of supplied history snapshots.
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

use super::{HistoryKind, HistoryOverrides, HistoryRecord, invalid};
use crate::ast::{ScalarValue, Value};
use crate::{AhclVersion, ConfigDocument, ConfigError, is_config_name, is_secure_https_url};
use crate::{parse_date_at, parse_repo_path, parse_version};
use ahcl_kit_core::UtcDate;
use std::collections::BTreeMap;
use url::Url;

const FIELDS: &[&str] = &[
    "id",
    "kind",
    "segment",
    "lineage",
    "effective-date",
    "effective-time",
    "covered-scope",
    "version",
    "from-version",
    "project",
    "canonical-repository",
    "authority",
    "signature",
    "official-version-url",
    "notice",
    "revision",
    "previous-segment",
    "previous-lineage",
    "non-ahcl-license",
    "title",
    "notes",
    "provenance",
    "evidence",
];

pub(super) fn overrides(
    document: &ConfigDocument,
    section: &str,
) -> Result<Option<HistoryOverrides>, ConfigError> {
    let supplied = HistoryOverrides {
        current_segment: optional_name(document, section, "current-segment")?,
        current_lineage: optional_name(document, section, "current-lineage")?,
        segment_start: optional_date(document, section, "segment-start")?,
        effective_date: optional_date(document, section, "effective-date")?,
        records: records(document, section)?,
    };
    if supplied == HistoryOverrides::default() {
        return Ok(None);
    }
    Ok(Some(supplied))
}

fn optional_name(
    document: &ConfigDocument,
    section: &str,
    key: &str,
) -> Result<Option<String>, ConfigError> {
    let value = document.optional_string(Some(section), key)?;
    if let Some(value) = &value {
        validate_name(&format!("{section}.{key}"), value)?;
    }
    Ok(value)
}

fn optional_date(
    document: &ConfigDocument,
    section: &str,
    key: &str,
) -> Result<Option<UtcDate>, ConfigError> {
    document
        .optional_string(Some(section), key)?
        .map(|value| parse_date_at(&value, &format!("{section}.{key}")))
        .transpose()
}

fn records(
    document: &ConfigDocument,
    section: &str,
) -> Result<Option<Vec<HistoryRecord>>, ConfigError> {
    let path = format!("{section}.history");
    match document
        .ast()
        .assignment(Some(section), "history")
        .map(|entry| &entry.value)
    {
        None => Ok(None),
        Some(Value::EmptyList) => Ok(Some(Vec::new())),
        Some(Value::ObjectList(values)) => values
            .iter()
            .enumerate()
            .map(|(index, fields)| parse_record(&format!("{path}[{index}]"), fields))
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(_) => invalid(&path, "must be an object list"),
    }
}

fn parse_record(
    path: &str,
    supplied: &BTreeMap<String, ScalarValue>,
) -> Result<HistoryRecord, ConfigError> {
    let fields = string_fields(path, supplied)?;
    required_fields(path, &fields, &["id", "kind", "segment", "covered-scope"])?;
    validate_record_names(path, &fields)?;
    validate_references(path, &fields)?;
    let record = HistoryRecord {
        kind: parse_kind(path, required(path, &fields, "kind")?)?,
        effective_date: fields
            .get("effective-date")
            .map(|value| parse_date_at(value, &format!("{path}.effective-date")))
            .transpose()?,
        version: optional_version(path, &fields, "version")?,
        from_version: optional_version(path, &fields, "from-version")?,
        fields,
    };
    validate_kind(path, &record)?;
    Ok(record)
}

fn string_fields(
    path: &str,
    supplied: &BTreeMap<String, ScalarValue>,
) -> Result<BTreeMap<String, String>, ConfigError> {
    supplied
        .iter()
        .map(|(key, value)| {
            let field_path = format!("{path}.{key}");
            if !FIELDS.contains(&key.as_str()) {
                return invalid(&field_path, "unknown history record field");
            }
            let ScalarValue::String(value) = value else {
                return invalid(&field_path, "must be a string");
            };
            validate_text(&field_path, key, value)?;
            Ok((key.clone(), value.clone()))
        })
        .collect()
}

fn validate_text(path: &str, key: &str, value: &str) -> Result<(), ConfigError> {
    if has_managed_marker(value) {
        return invalid(
            path,
            "cannot contain AHCL notice delimiters or managed markers",
        );
    }
    let multiline = [
        "authority",
        "signature",
        "notes",
        "provenance",
        "evidence",
        "covered-scope",
    ]
    .contains(&key);
    if value
        .chars()
        .any(|character| invalid_character(character, multiline))
    {
        return invalid(
            path,
            "contains a control character or a line break in an identity field",
        );
    }
    Ok(())
}

fn invalid_character(character: char, multiline: bool) -> bool {
    if multiline && matches!(character, '\n' | '\r' | '\t') {
        return false;
    }
    character.is_control() || matches!(character, '\u{2028}' | '\u{2029}')
}

fn has_managed_marker(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    [
        "AHCL KIT MANAGED",
        "AHCL KIT HISTORY",
        "----- BEGIN AHCL NOTICE -----",
        "----- END AHCL NOTICE -----",
    ]
    .iter()
    .any(|marker| upper.contains(marker))
}

fn validate_record_names(path: &str, fields: &BTreeMap<String, String>) -> Result<(), ConfigError> {
    for key in [
        "id",
        "segment",
        "lineage",
        "previous-segment",
        "previous-lineage",
    ] {
        if let Some(value) = fields.get(key) {
            validate_name(&format!("{path}.{key}"), value)?;
        }
    }
    Ok(())
}

fn validate_name(path: &str, value: &str) -> Result<(), ConfigError> {
    if !is_config_name(value) {
        return invalid(
            path,
            "must be a non-empty configuration name using letters, digits, '_' or '-'",
        );
    }
    Ok(())
}

fn parse_kind(path: &str, value: &str) -> Result<HistoryKind, ConfigError> {
    match value {
        "initial-adoption" => Ok(HistoryKind::InitialAdoption),
        "version-adoption" => Ok(HistoryKind::VersionAdoption),
        "interruption" => Ok(HistoryKind::Interruption),
        "readoption" => Ok(HistoryKind::Readoption),
        "record" => Ok(HistoryKind::Record),
        _ => invalid(&format!("{path}.kind"), "unsupported history kind"),
    }
}

fn optional_version(
    path: &str,
    fields: &BTreeMap<String, String>,
    key: &str,
) -> Result<Option<AhclVersion>, ConfigError> {
    fields
        .get(key)
        .map(|value| parse_version(value, &format!("{path}.{key}")))
        .transpose()
}

fn required<'a>(
    path: &str,
    fields: &'a BTreeMap<String, String>,
    key: &str,
) -> Result<&'a str, ConfigError> {
    fields
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ConfigError::InvalidValue {
            path: format!("{path}.{key}"),
            message: "is required and must not be empty".to_owned(),
        })
}

fn required_fields(
    path: &str,
    fields: &BTreeMap<String, String>,
    keys: &[&str],
) -> Result<(), ConfigError> {
    for key in keys {
        required(path, fields, key)?;
    }
    Ok(())
}

fn validate_kind(path: &str, record: &HistoryRecord) -> Result<(), ConfigError> {
    if record.kind != HistoryKind::Record {
        required(path, &record.fields, "effective-date")?;
    }
    match record.kind {
        HistoryKind::InitialAdoption => required_fields(path, &record.fields, &["version"]),
        HistoryKind::Readoption => validate_readoption(path, record),
        HistoryKind::VersionAdoption => validate_version_adoption(path, record),
        HistoryKind::Interruption => validate_interruption(path, &record.fields),
        HistoryKind::Record => required_fields(path, &record.fields, &["title", "notes"]),
    }
}

fn validate_readoption(path: &str, record: &HistoryRecord) -> Result<(), ConfigError> {
    required_fields(path, &record.fields, &["version"])?;
    if record.fields.contains_key("previous-lineage")
        && !record.fields.contains_key("previous-segment")
    {
        return invalid(
            &format!("{path}.previous-lineage"),
            "requires previous-segment to identify a complete predecessor group",
        );
    }
    Ok(())
}

fn validate_version_adoption(path: &str, record: &HistoryRecord) -> Result<(), ConfigError> {
    required_fields(
        path,
        &record.fields,
        &[
            "version",
            "from-version",
            "project",
            "canonical-repository",
            "authority",
            "signature",
            "official-version-url",
            "notice",
        ],
    )?;
    if record.version <= record.from_version {
        return invalid(
            &format!("{path}.version"),
            "version-adoption must adopt a strictly later version than from-version",
        );
    }
    Ok(())
}

fn validate_interruption(path: &str, fields: &BTreeMap<String, String>) -> Result<(), ConfigError> {
    required_fields(path, fields, &["non-ahcl-license", "authority"])?;
    if is_ahcl_license(&fields["non-ahcl-license"]) {
        return invalid(
            &format!("{path}.non-ahcl-license"),
            "must identify a license other than AHCL",
        );
    }
    Ok(())
}

fn is_ahcl_license(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value == "ahcl"
        || [
            "ahcl ",
            "ahcl-",
            "licenseref-ahcl-",
            "aperip heimdall commons license",
        ]
        .iter()
        .any(|prefix| value.starts_with(prefix))
}

fn validate_references(path: &str, fields: &BTreeMap<String, String>) -> Result<(), ConfigError> {
    if let Some(value) = fields.get("canonical-repository") {
        validate_https(&format!("{path}.canonical-repository"), value)?;
    }
    if let Some(value) = fields.get("official-version-url") {
        validate_official_url(&format!("{path}.official-version-url"), value)?;
    }
    if let Some(value) = fields.get("notice") {
        validate_notice(&format!("{path}.notice"), value)?;
    }
    Ok(())
}

fn validate_https(path: &str, value: &str) -> Result<(), ConfigError> {
    if !is_secure_https_url(value) {
        return invalid(
            path,
            "must be an absolute HTTPS URL without credentials, query, or fragment",
        );
    }
    Ok(())
}

fn validate_official_url(path: &str, value: &str) -> Result<(), ConfigError> {
    validate_https(path, value)?;
    if !Url::parse(value).is_ok_and(|url| {
        url.host_str() == Some("ahcl.aperip.com") && url.port_or_known_default() == Some(443)
    }) {
        return invalid(
            path,
            "must be under the official https://ahcl.aperip.com origin",
        );
    }
    Ok(())
}

fn validate_notice(path: &str, value: &str) -> Result<(), ConfigError> {
    if Url::parse(value).is_ok() {
        return validate_https(path, value);
    }
    parse_repo_path(path, value)?;
    Ok(())
}
