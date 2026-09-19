// crates/ahcl-kit-cli/src/dispatch.rs - Command dispatch and runtime orchestration.
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

use crate::{
    AdapterKind, CommandReport, CommandRuntime, OutputChange, OutputChangeKind, OutputDiagnostic,
    OutputSeverity, ParsedInvocation, PlanRequest, PlanScope, ProjectReport, ProjectStatus,
    ResolvedAdapter, RuntimeError, RuntimePlan, execute_batch,
};
use ahcl_kit_config::{EffectiveConfig, Language, ProjectIdentity};
use ahcl_kit_core::{ChangeKind, CommandId, Diagnostic, DiagnosticSeverity, ProjectRoot, UtcDate};
use ahcl_kit_license::VerifiedLicense;
use ahcl_kit_materials::ManagedRemoval;
use std::path::Path;

pub fn run(invocation: &ParsedInvocation, runtime: &mut dyn CommandRuntime) -> CommandReport {
    let projects = match invocation.resolve_projects() {
        Ok(projects) => projects,
        Err(error) => {
            return command_error(invocation, error.code(), error.to_string());
        }
    };
    let identity = match project_identity(invocation) {
        Ok(identity) => identity,
        Err(error) => return command_error(invocation, error.code(), error.to_string()),
    };
    let current_date = if invocation.command_id() == CommandId::ProjectInit {
        match runtime.current_utc_date() {
            Ok(date) => Some(date),
            Err(error) => return command_runtime_error(invocation, &error),
        }
    } else {
        None
    };

    let batch = execute_batch(projects, invocation.fail_fast(), |path| {
        execute_project(invocation, runtime, path, identity.as_ref(), current_date)
    });
    let reports = batch
        .items
        .into_iter()
        .map(|item| match item.result {
            Ok(value) => value.value,
            Err(error) => runtime_error_report(item.project, &error),
        })
        .collect();
    CommandReport::new(
        invocation.command_id(),
        invocation.invocation_name(),
        reports,
        Vec::new(),
    )
}

fn execute_project(
    invocation: &ParsedInvocation,
    runtime: &mut dyn CommandRuntime,
    path: &Path,
    identity: Option<&ProjectIdentity>,
    current_date: Option<UtcDate>,
) -> Result<crate::BatchValue<ProjectReport>, RuntimeError> {
    let project = ProjectRoot::new(path.to_path_buf())
        .map_err(|error| RuntimeError::with_source("cli.project_root", error))?;
    let command_id = invocation.command_id();
    if matches!(
        command_id,
        CommandId::ConfigValidate | CommandId::ConfigShowResolved
    ) {
        runtime.load_config(&project)?;
        return Ok(crate::BatchValue::success(ProjectReport::new(
            path.to_path_buf(),
            ProjectStatus::Success,
            Vec::new(),
            Vec::new(),
        )));
    }

    let scopes = scopes(command_id);
    let config = load_config(invocation, runtime, &project, identity, current_date)?;
    let license = fetch_license(command_id, runtime, config.as_ref())?;
    let adapters = resolve_adapters(command_id, runtime, &project, config.as_ref())?;
    let request = PlanRequest::new(
        scopes,
        config.as_ref(),
        license.as_ref(),
        &adapters,
        current_date,
        invocation.force(),
        identity,
    );
    let desired = runtime.plan_project(&project, request)?;
    let compared = runtime.compare_project(&project, desired)?;
    let planned_changes = output_changes(&compared);
    let diagnostics = output_diagnostics(compared.diagnostics());

    if command_id == CommandId::ProjectCheck {
        let drift = !compared.is_empty();
        let report = ProjectReport::new(
            path.to_path_buf(),
            if drift {
                ProjectStatus::Drift
            } else {
                ProjectStatus::Success
            },
            diagnostics,
            planned_changes,
        );
        return if drift {
            Ok(crate::BatchValue::drift(report))
        } else {
            Ok(crate::BatchValue::success(report))
        };
    }

    if !invocation.dry_run() {
        runtime.apply_project(&project, &compared)?;
    }
    Ok(crate::BatchValue::success(ProjectReport::new(
        path.to_path_buf(),
        ProjectStatus::Success,
        diagnostics,
        planned_changes,
    )))
}

fn project_identity(
    invocation: &ParsedInvocation,
) -> Result<Option<ProjectIdentity>, RuntimeError> {
    let Some(identity) = invocation.project_identity() else {
        return Ok(None);
    };
    let any = identity.name.is_some()
        || identity.repository.is_some()
        || !identity.right_holders.is_empty();
    if !any {
        return Ok(None);
    }
    let complete = identity
        .name
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
        && identity
            .repository
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        && !identity.right_holders.is_empty()
        && identity
            .right_holders
            .iter()
            .all(|holder| !holder.trim().is_empty());
    if !complete {
        return Err(RuntimeError::operation("config.identity_required"));
    }
    match (identity.name, identity.repository) {
        (Some(name), Some(repository)) => Ok(Some(ProjectIdentity::new(
            name,
            repository,
            identity.right_holders,
        ))),
        _ => Err(RuntimeError::operation("config.identity_required")),
    }
}

fn load_config(
    invocation: &ParsedInvocation,
    runtime: &mut dyn CommandRuntime,
    project: &ProjectRoot,
    identity: Option<&ProjectIdentity>,
    current_date: Option<UtcDate>,
) -> Result<Option<EffectiveConfig>, RuntimeError> {
    match invocation.command_id() {
        CommandId::ConfigInit => Ok(None),
        CommandId::ProjectInit => runtime
            .prepare_project_init(
                project,
                identity,
                current_date.ok_or_else(|| RuntimeError::operation("cli.current_date"))?,
            )
            .map(Some),
        _ => runtime.load_config(project).map(Some),
    }
}

fn fetch_license(
    command_id: CommandId,
    runtime: &mut dyn CommandRuntime,
    config: Option<&EffectiveConfig>,
) -> Result<Option<VerifiedLicense>, RuntimeError> {
    if !matches!(
        command_id,
        CommandId::ProjectInit
            | CommandId::ProjectGenerate
            | CommandId::ProjectCheck
            | CommandId::LicenseSync
    ) {
        return Ok(None);
    }
    let config = config.ok_or_else(|| RuntimeError::operation("config.unavailable"))?;
    runtime
        .fetch_official_license(config.license().version())
        .map(Some)
}

fn resolve_adapters(
    command_id: CommandId,
    runtime: &mut dyn CommandRuntime,
    project: &ProjectRoot,
    config: Option<&EffectiveConfig>,
) -> Result<Vec<ResolvedAdapter>, RuntimeError> {
    if !matches!(
        command_id,
        CommandId::ProjectGenerate
            | CommandId::ProjectCheck
            | CommandId::DependencyGenerate
            | CommandId::ThirdPartyGenerate
    ) {
        return Ok(Vec::new());
    }
    let config = config.ok_or_else(|| RuntimeError::operation("config.unavailable"))?;
    config
        .languages()
        .iter()
        .map(|language| match language {
            Language::Rust => runtime
                .resolve_adapter(AdapterKind::Cargo, project, config)
                .map(|graph| ResolvedAdapter::new(AdapterKind::Cargo, graph)),
        })
        .collect()
}

fn scopes(command_id: CommandId) -> &'static [PlanScope] {
    match command_id {
        CommandId::ConfigInit => &[PlanScope::ConfigInit],
        CommandId::ConfigValidate | CommandId::ConfigShowResolved => &[],
        CommandId::ProjectInit => &[PlanScope::ProjectInit],
        CommandId::ProjectGenerate | CommandId::ProjectCheck => &[
            PlanScope::ProjectFiles,
            PlanScope::Dependencies,
            PlanScope::ThirdParty,
        ],
        CommandId::LicenseSync => &[PlanScope::License],
        CommandId::DependencyGenerate => &[PlanScope::Dependencies],
        CommandId::ThirdPartyGenerate => &[PlanScope::ThirdParty],
    }
}

fn output_changes(plan: &RuntimePlan) -> Vec<OutputChange> {
    let mut changes = plan
        .changes()
        .changes()
        .into_iter()
        .map(|change| {
            let kind = match change.kind() {
                ChangeKind::Create => OutputChangeKind::Create,
                ChangeKind::Replace => OutputChangeKind::Replace,
                ChangeKind::Remove => OutputChangeKind::Remove,
            };
            OutputChange::new(change.path().as_str(), kind)
        })
        .collect::<Vec<_>>();
    if let Some(materials_directory) = plan.managed_materials_directory() {
        let base = format!("{}/THIRD-PARTY-LICENSES", materials_directory.as_str());
        changes.extend(plan.managed_removals().iter().map(|removal| {
            let path = match removal {
                ManagedRemoval::Evidence {
                    package_directory,
                    evidence_basename,
                } => format!("{base}/{package_directory}/{evidence_basename}"),
                ManagedRemoval::PackageDirectory { package_directory } => {
                    format!("{base}/{package_directory}")
                }
            };
            OutputChange::new(path, OutputChangeKind::Remove)
        }));
    }
    changes
}

fn output_diagnostics(diagnostics: &[Diagnostic]) -> Vec<OutputDiagnostic> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            let severity = match diagnostic.severity {
                DiagnosticSeverity::Error => OutputSeverity::Error,
                DiagnosticSeverity::Warning => OutputSeverity::Warning,
                DiagnosticSeverity::Information => OutputSeverity::Information,
            };
            OutputDiagnostic::new(
                diagnostic.code.as_str(),
                severity,
                diagnostic.message.clone(),
            )
        })
        .collect()
}

fn runtime_error_report(path: std::path::PathBuf, error: &RuntimeError) -> ProjectReport {
    ProjectReport::new(
        path,
        ProjectStatus::Error,
        vec![runtime_diagnostic(error)],
        Vec::new(),
    )
}

fn command_runtime_error(invocation: &ParsedInvocation, error: &RuntimeError) -> CommandReport {
    CommandReport::new(
        invocation.command_id(),
        invocation.invocation_name(),
        Vec::new(),
        vec![runtime_diagnostic(error)],
    )
}

fn command_error(
    invocation: &ParsedInvocation,
    code: impl Into<String>,
    message: impl Into<String>,
) -> CommandReport {
    CommandReport::new(
        invocation.command_id(),
        invocation.invocation_name(),
        Vec::new(),
        vec![OutputDiagnostic::new(code, OutputSeverity::Error, message)],
    )
}

fn runtime_diagnostic(error: &RuntimeError) -> OutputDiagnostic {
    OutputDiagnostic::new(
        error.code(),
        OutputSeverity::Error,
        "runtime operation failed",
    )
}
