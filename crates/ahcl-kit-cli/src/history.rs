// crates/ahcl-kit-cli/src/history.rs - Local configured licensing history display.
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

use crate::{RuntimeError, dispatch::config_value_to_json};
use ahcl_kit_config::{EffectiveConfig, inline_text, matching_scope};
use serde_json::{Value, json};

pub(crate) fn configured_history(config: &EffectiveConfig) -> Result<Value, RuntimeError> {
    let mut scopes = vec![scope_value("project", config)];
    for binding in config.bindings() {
        let components = binding
            .component_configs(config)
            .map_err(|error| RuntimeError::with_source("config.history", error))?;
        for (id, component) in components {
            scopes.push(scope_value(
                &format!("{}:{id}", binding.language_id()),
                &component,
            ));
        }
    }
    Ok(json!({ "source": "configuration", "scopes": scopes }))
}

fn scope_value(id: &str, config: &EffectiveConfig) -> Value {
    json!({
        "id": id,
        "project": config.project().name(),
        "canonical_repository": config.project().canonical_repository(),
        "covered_scope": config.license().covered_scope(),
        "initial_adoption_date": config.project().adoption_date().map(|date| date.to_string()),
        "version": config.license().version().as_str(),
        "enabled": config.license().enabled(),
        "history": config_value_to_json(&config.license().history().resolved_value()),
    })
}

pub(crate) fn render_text(value: &Value) -> String {
    let mut output = String::from("  Configured AHCL licensing history\n");
    let Some(scopes) = value.get("scopes").and_then(Value::as_array) else {
        return output;
    };
    for scope in scopes {
        render_scope(&mut output, scope);
    }
    output
}

fn render_scope(output: &mut String, scope: &Value) {
    output.push_str(&format!(
        "\n  Scope {}: {}\n",
        text(&scope["id"]),
        scope_text(&scope["covered_scope"])
    ));
    output.push_str(&format!(
        "    Project: {}\n    Canonical repository: {}\n",
        text(&scope["project"]),
        text(&scope["canonical_repository"])
    ));
    output.push_str(&format!(
        "    Initial AHCL adoption: {}\n    Configured version: AHCL {} (enabled: {})\n",
        text(&scope["initial_adoption_date"]),
        text(&scope["version"]),
        text(&scope["enabled"])
    ));
    let history = &scope["history"];
    output.push_str(&format!(
        "    Current segment: {}\n    Revision lineage: {}\n    Segment beginning: {}\n    Version Effective Date: {}\n",
        text(&history["current_segment"]),
        text(&history["current_lineage"]),
        text(&history["segment_start"]),
        text(&history["effective_date"])
    ));
    render_records(output, history.get("records").and_then(Value::as_array));
}

fn render_records(output: &mut String, records: Option<&Vec<Value>>) {
    let Some(records) = records.filter(|records| !records.is_empty()) else {
        output.push_str(
            "    No history records configured. Existing document records remain preserved.\n",
        );
        return;
    };
    for record in records {
        output.push_str("\n    Record:\n");
        render_record(output, record);
    }
}

fn render_record(output: &mut String, record: &Value) {
    let Some(fields) = record.as_object() else {
        return;
    };
    for (key, value) in fields {
        output.push_str(&format!(
            "      {}: {}\n",
            key.replace('_', " "),
            text(value)
        ));
    }
}

fn scope_text(value: &Value) -> String {
    value
        .as_str()
        .map(matching_scope)
        .map(inline_text)
        .unwrap_or_else(|| text(value))
}

fn text(value: &Value) -> String {
    match value {
        Value::String(value) if !value.is_empty() => inline_text(value),
        Value::Null => "unknown".to_owned(),
        Value::String(_) => "(empty)".to_owned(),
        _ => value.to_string(),
    }
}
