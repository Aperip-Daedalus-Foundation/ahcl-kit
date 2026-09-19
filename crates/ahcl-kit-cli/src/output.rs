// crates/ahcl-kit-cli/src/output.rs - Command report models and text and JSON rendering.
//
// Copyright (C) 2026 Aperip Daedalus Foundation. All rights reserved.
//
// This file is part of AHCL Kit and is provided under version 1.1 of the
// Aperip Heimdall Commons License (AHCL). The applicable version is also subject
// to the AHCL provisions concerning Continuous AHCL Licensing Segments and
// migration to later official versions.
//
// After having a reasonable opportunity to read AHCL, all applicable Additional
// Restrictions, and all version notices, a person accepts the corresponding terms,
// to the extent permitted by applicable law, by using, copying, modifying, building,
// using this file as a dependency, deploying, distributing, or operating this file
// over a network.
//
// Official AHCL text and public notices:          https://ahcl.aperip.com
// AHCL Materials Directory:                       .ahcl/
// Repository official or recognized AHCL copy:   .ahcl/AHCL-1.1.md
// Project canonical repository:                   https://github.com/Aperip-Daedalus-Foundation/ahcl-kit
// AHCL origin and project notice:                 .ahcl/AHCL-PROJECT-NOTICE.md
// AHCL Version Adoption records:                  .ahcl/AHCL-VERSION-ADOPTION.md
// Complete Corresponding Source and history:      .ahcl/AHCL-SOURCE.md
// Dependencies, Referenced Materials, and licenses:
//                                                    .ahcl/AHCL-DEPENDENCIES.md
//
// SPDX-License-Identifier: LicenseRef-AHCL-1.1

use crate::ParsedInvocation;
use ahcl_kit_core::CommandId;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputSeverity {
    Error,
    Warning,
    Information,
}

impl OutputSeverity {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Information => "information",
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OutputDiagnostic {
    pub code: String,
    pub severity: OutputSeverity,
    pub message: String,
}

impl OutputDiagnostic {
    pub fn new(
        code: impl Into<String>,
        severity: OutputSeverity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity,
            message: message.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputChangeKind {
    Create,
    Replace,
    Remove,
}

impl OutputChangeKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Replace => "replace",
            Self::Remove => "remove",
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct OutputChange {
    pub path: String,
    pub kind: OutputChangeKind,
}

impl OutputChange {
    pub fn new(path: impl Into<String>, kind: OutputChangeKind) -> Self {
        Self {
            path: path.into(),
            kind,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
    Success,
    Drift,
    Error,
}

impl ProjectStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Drift => "drift",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProjectReport {
    pub project: String,
    pub status: ProjectStatus,
    pub diagnostics: Vec<OutputDiagnostic>,
    pub planned_changes: Vec<OutputChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_config: Option<serde_json::Value>,
}

impl ProjectReport {
    pub fn new(
        project: PathBuf,
        status: ProjectStatus,
        mut diagnostics: Vec<OutputDiagnostic>,
        mut planned_changes: Vec<OutputChange>,
    ) -> Self {
        diagnostics.sort();
        planned_changes.sort();
        Self {
            project: display_path(&project),
            status,
            diagnostics,
            planned_changes,
            resolved_config: None,
        }
    }

    pub(crate) fn with_resolved_config(mut self, resolved_config: serde_json::Value) -> Self {
        self.resolved_config = Some(resolved_config);
        self
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct AggregateCounts {
    pub projects: u64,
    pub success: u64,
    pub drift: u64,
    pub errors: u64,
    pub diagnostics: u64,
    pub planned_changes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CommandReport {
    pub schema: u32,
    pub command_id: String,
    pub invocation_name: String,
    pub projects: Vec<ProjectReport>,
    pub diagnostics: Vec<OutputDiagnostic>,
    pub aggregate: AggregateCounts,
}

impl CommandReport {
    pub fn new(
        command_id: CommandId,
        invocation_name: impl Into<String>,
        mut projects: Vec<ProjectReport>,
        mut diagnostics: Vec<OutputDiagnostic>,
    ) -> Self {
        projects.sort_by(|left, right| {
            path_text_key(&left.project).cmp(&path_text_key(&right.project))
        });
        diagnostics.sort();
        let mut aggregate = AggregateCounts {
            projects: projects.len() as u64,
            diagnostics: diagnostics.len() as u64,
            ..AggregateCounts::default()
        };
        for project in &projects {
            match project.status {
                ProjectStatus::Success => aggregate.success += 1,
                ProjectStatus::Drift => aggregate.drift += 1,
                ProjectStatus::Error => aggregate.errors += 1,
            }
            aggregate.diagnostics += project.diagnostics.len() as u64;
            aggregate.planned_changes += project.planned_changes.len() as u64;
        }
        aggregate.errors += diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == OutputSeverity::Error)
            .count() as u64;
        Self {
            schema: 1,
            command_id: command_id.as_str().to_owned(),
            invocation_name: invocation_name.into(),
            projects,
            diagnostics,
            aggregate,
        }
    }

    pub fn exit_code(&self) -> u8 {
        if self.aggregate.errors > 0 {
            1
        } else if self.aggregate.drift > 0 {
            2
        } else {
            0
        }
    }
}

pub fn not_wired_report(invocation: &ParsedInvocation) -> CommandReport {
    CommandReport::new(
        invocation.command_id(),
        invocation.invocation_name(),
        Vec::new(),
        vec![OutputDiagnostic::new(
            "cli.not_wired",
            OutputSeverity::Error,
            "command execution is not wired",
        )],
    )
}

pub fn render_json(report: &CommandReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report).map(|mut rendered| {
        rendered.push('\n');
        rendered
    })
}

pub fn render_text(report: &CommandReport) -> String {
    let mut rendered = format!(
        "command: {}\ninvocation: {}\nprojects: {} (success {}, drift {}, errors {})\n",
        report.command_id,
        report.invocation_name,
        report.aggregate.projects,
        report.aggregate.success,
        report.aggregate.drift,
        report.aggregate.errors,
    );
    for diagnostic in &report.diagnostics {
        rendered.push_str(&format!(
            "[{}] {}: {}\n",
            diagnostic.severity.as_str(),
            diagnostic.code,
            diagnostic.message
        ));
    }
    for project in &report.projects {
        rendered.push_str(&format!(
            "{}: {}\n",
            project.project,
            project.status.as_str()
        ));
        if let Some(resolved_config) = &project.resolved_config {
            rendered.push_str(&format!("  resolved_config: {resolved_config}\n"));
        }
        for change in &project.planned_changes {
            rendered.push_str(&format!("  {} {}\n", change.kind.as_str(), change.path));
        }
        for diagnostic in &project.diagnostics {
            rendered.push_str(&format!(
                "  [{}] {}: {}\n",
                diagnostic.severity.as_str(),
                diagnostic.code,
                diagnostic.message
            ));
        }
    }
    rendered
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn path_text_key(path: &str) -> String {
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.to_owned()
    }
}
