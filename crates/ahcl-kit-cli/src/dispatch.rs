// crates/ahcl-kit-cli/src/dispatch.rs - Command dispatch and runtime orchestration.
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
    AdapterKind, CommandReport, CommandRuntime, LanguageAdapterRegistry, OutputChange,
    OutputChangeKind, OutputDiagnostic, OutputSeverity, ParsedInvocation, PlanRequest, PlanScope,
    ProjectIdentityArgs, ProjectReport, ProjectStatus, ResolvedAdapter, RuntimeError, RuntimePlan,
    execute_batch,
};
use ahcl_kit_config::{ConfigValue, EffectiveConfig, ProjectIdentity};
use ahcl_kit_core::{ChangeKind, CommandId, Diagnostic, DiagnosticSeverity, ProjectRoot, UtcDate};
use ahcl_kit_license::VerifiedLicense;
use ahcl_kit_materials::ManagedRemoval;
use std::collections::BTreeSet;
use std::path::Path;

pub fn run(invocation: &ParsedInvocation, runtime: &mut dyn CommandRuntime) -> CommandReport {
    match dispatched(invocation, runtime) {
        Ok(report) => report,
        Err(report) => report,
    }
}

fn dispatched(
    invocation: &ParsedInvocation,
    runtime: &mut dyn CommandRuntime,
) -> Result<CommandReport, CommandReport> {
    let projects = resolved_projects(invocation)?;
    let identity = resolved_identity(invocation)?;
    let current_date = init_date(invocation, runtime)?;
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
    Ok(CommandReport::new(
        invocation.command_id(),
        invocation.invocation_name(),
        reports,
        Vec::new(),
    ))
}

fn resolved_projects(
    invocation: &ParsedInvocation,
) -> Result<Vec<std::path::PathBuf>, CommandReport> {
    invocation
        .resolve_projects()
        .map_err(|error| command_error(invocation, error.code(), error.to_string()))
}

fn resolved_identity(
    invocation: &ParsedInvocation,
) -> Result<Option<ProjectIdentity>, CommandReport> {
    project_identity(invocation)
        .map_err(|error| command_error(invocation, error.code(), error.to_string()))
}

fn init_date(
    invocation: &ParsedInvocation,
    runtime: &mut dyn CommandRuntime,
) -> Result<Option<UtcDate>, CommandReport> {
    if invocation.command_id() != CommandId::ProjectInit {
        return Ok(None);
    }
    match runtime.current_utc_date() {
        Ok(date) => Ok(Some(date)),
        Err(error) => Err(command_runtime_error(invocation, &error)),
    }
}

fn execute_project(
    invocation: &ParsedInvocation,
    runtime: &mut dyn CommandRuntime,
    path: &Path,
    identity: Option<&ProjectIdentity>,
    current_date: Option<UtcDate>,
) -> Result<crate::BatchValue<ProjectReport>, RuntimeError> {
    let project = open_project(path)?;
    let command_id = invocation.command_id();
    if let Some(report) = config_only_report(command_id, runtime, &project, path)? {
        return Ok(report);
    }
    apply_planned(invocation, runtime, path, &project, identity, current_date)
}

fn open_project(path: &Path) -> Result<ProjectRoot, RuntimeError> {
    ProjectRoot::new(path.to_path_buf())
        .map_err(|error| RuntimeError::with_source("cli.project_root", error))
}

fn config_only_report(
    command_id: CommandId,
    runtime: &mut dyn CommandRuntime,
    project: &ProjectRoot,
    path: &Path,
) -> Result<Option<crate::BatchValue<ProjectReport>>, RuntimeError> {
    if command_id == CommandId::ConfigValidate {
        return validate_report(runtime, project, path).map(Some);
    }
    if command_id == CommandId::ConfigShowResolved {
        return show_report(runtime, project, path).map(Some);
    }
    Ok(None)
}

fn validate_report(
    runtime: &mut dyn CommandRuntime,
    project: &ProjectRoot,
    path: &Path,
) -> Result<crate::BatchValue<ProjectReport>, RuntimeError> {
    runtime.load_config(project)?;
    Ok(crate::BatchValue::success(ProjectReport::new(
        path.to_path_buf(),
        ProjectStatus::Success,
        Vec::new(),
        Vec::new(),
    )))
}

fn show_report(
    runtime: &mut dyn CommandRuntime,
    project: &ProjectRoot,
    path: &Path,
) -> Result<crate::BatchValue<ProjectReport>, RuntimeError> {
    let config = runtime.load_config(project)?;
    Ok(crate::BatchValue::success(
        ProjectReport::new(
            path.to_path_buf(),
            ProjectStatus::Success,
            Vec::new(),
            Vec::new(),
        )
        .with_resolved_config(resolved_config(&config)),
    ))
}

fn apply_planned(
    invocation: &ParsedInvocation,
    runtime: &mut dyn CommandRuntime,
    path: &Path,
    project: &ProjectRoot,
    identity: Option<&ProjectIdentity>,
    current_date: Option<UtcDate>,
) -> Result<crate::BatchValue<ProjectReport>, RuntimeError> {
    let command_id = invocation.command_id();
    let planned = plan_compared(invocation, runtime, project, identity, current_date)?;
    if command_id == CommandId::ProjectCheck {
        return Ok(check_value(path, planned));
    }
    if !invocation.dry_run() {
        runtime.apply_project(project, &planned.compared)?;
    }
    Ok(crate::BatchValue::success(ProjectReport::new(
        path.to_path_buf(),
        ProjectStatus::Success,
        planned.diagnostics,
        planned.planned_changes,
    )))
}

struct PlannedProject {
    compared: RuntimePlan,
    diagnostics: Vec<OutputDiagnostic>,
    planned_changes: Vec<OutputChange>,
}

fn plan_compared(
    invocation: &ParsedInvocation,
    runtime: &mut dyn CommandRuntime,
    project: &ProjectRoot,
    identity: Option<&ProjectIdentity>,
    current_date: Option<UtcDate>,
) -> Result<PlannedProject, RuntimeError> {
    let command_id = invocation.command_id();
    let config = load_config(invocation, runtime, project, identity, current_date)?;
    let license = fetch_license(command_id, runtime, config.as_ref())?;
    let adapters = resolve_adapters(command_id, runtime, project, config.as_ref())?;
    let request = PlanRequest::new(
        scopes(command_id),
        config.as_ref(),
        license.as_ref(),
        &adapters,
        current_date,
        invocation.force(),
    )
    .with_identity(identity);
    let desired = runtime.plan_project(project, request)?;
    let compared = runtime.compare_project(project, desired)?;
    Ok(PlannedProject {
        diagnostics: output_diagnostics(compared.diagnostics()),
        planned_changes: output_changes(&compared),
        compared,
    })
}

fn check_value(path: &Path, planned: PlannedProject) -> crate::BatchValue<ProjectReport> {
    let drift = !planned.compared.is_empty();
    let report = ProjectReport::new(
        path.to_path_buf(),
        if drift {
            ProjectStatus::Drift
        } else {
            ProjectStatus::Success
        },
        planned.diagnostics,
        planned.planned_changes,
    );
    if drift {
        crate::BatchValue::drift(report)
    } else {
        crate::BatchValue::success(report)
    }
}

fn project_identity(
    invocation: &ParsedInvocation,
) -> Result<Option<ProjectIdentity>, RuntimeError> {
    let Some(identity) = invocation.project_identity() else {
        return Ok(None);
    };
    if !identity_present(&identity) {
        return Ok(None);
    }
    if !identity_complete(&identity) {
        return Err(RuntimeError::operation("config.identity_required"));
    }
    identity_value(identity)
}

fn identity_present(identity: &ProjectIdentityArgs) -> bool {
    identity.name.is_some() || identity.repository.is_some() || !identity.right_holders.is_empty()
}

fn identity_complete(identity: &ProjectIdentityArgs) -> bool {
    nonempty_text(identity.name.as_deref())
        && nonempty_text(identity.repository.as_deref())
        && holders_present(&identity.right_holders)
}

fn nonempty_text(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn holders_present(holders: &[String]) -> bool {
    !holders.is_empty() && holders.iter().all(|holder| !holder.trim().is_empty())
}

fn identity_value(identity: ProjectIdentityArgs) -> Result<Option<ProjectIdentity>, RuntimeError> {
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
    if !adapter_command(command_id) {
        return Ok(Vec::new());
    }
    let config = config.ok_or_else(|| RuntimeError::operation("config.unavailable"))?;
    collect_adapters(runtime, project, config)
}

fn adapter_command(command_id: CommandId) -> bool {
    matches!(
        command_id,
        CommandId::ProjectGenerate
            | CommandId::ProjectCheck
            | CommandId::DependencyGenerate
            | CommandId::ThirdPartyGenerate
    )
}

fn collect_adapters(
    runtime: &mut dyn CommandRuntime,
    project: &ProjectRoot,
    config: &EffectiveConfig,
) -> Result<Vec<ResolvedAdapter>, RuntimeError> {
    let registry = LanguageAdapterRegistry::new(runtime.ecosystems().hosts());
    let languages = config.languages().to_vec();
    let mut kinds = BTreeSet::new();
    let mut adapters = Vec::with_capacity(config.languages().len());
    let mut accum = AdapterAccum {
        runtime,
        project,
        config,
        kinds: &mut kinds,
        adapters: &mut adapters,
    };
    for language in &languages {
        remember_adapter(&mut accum, &registry, language)?;
    }
    Ok(adapters)
}

struct AdapterAccum<'a> {
    runtime: &'a mut dyn CommandRuntime,
    project: &'a ProjectRoot,
    config: &'a EffectiveConfig,
    kinds: &'a mut BTreeSet<AdapterKind>,
    adapters: &'a mut Vec<ResolvedAdapter>,
}

fn remember_adapter(
    accum: &mut AdapterAccum<'_>,
    registry: &LanguageAdapterRegistry,
    language: &str,
) -> Result<(), RuntimeError> {
    let adapter = registry.adapter_for(language)?;
    if !accum.kinds.insert(adapter) {
        return Err(RuntimeError::operation("cli.adapter_duplicate"));
    }
    let graph = accum
        .runtime
        .resolve_adapter(adapter, accum.project, accum.config)?;
    accum.adapters.push(ResolvedAdapter::new(adapter, graph));
    Ok(())
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
                    ..
                } => format!("{base}/{package_directory}/{evidence_basename}"),
                ManagedRemoval::PackageDirectory { package_directory } => {
                    format!("{base}/{package_directory}")
                }
                ManagedRemoval::LegacyState { .. } => {
                    format!("{base}/.ahcl-kit-state.json")
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

fn resolved_config(config: &EffectiveConfig) -> serde_json::Value {
    let project = config.project();
    let limits = config.limits();
    let mut value = serde_json::json!({
        "schema": config.schema(),
        "materials_directory": config.materials_directory().as_str(),
        "languages": config.languages(),
        "project": {
            "name": project.name(),
            "canonical_repository": project.canonical_repository(),
            "canonical_branch": project.canonical_branch(),
            "right_holders": project.right_holders(),
            "contact": project.contact(),
            "adoption_date": project.adoption_date().map(|date| date.to_string()),
        },
        "license": {
            "version": config.license().version().as_str(),
            "enabled": config.license().enabled(),
            "covered_scope": config.license().covered_scope(),
            "special_authorization_channel": config.license().special_authorization_channel(),
        },
        "generation": {
            "strict_license_files": config.generation().strict_license_files(),
        },
    });
    let Some(object) = value.as_object_mut() else {
        return value;
    };
    for (key, section) in config.resolved_sections() {
        object.insert(key, config_value_to_json(&section));
    }
    object.insert(
        "limits".to_owned(),
        serde_json::json!({
            "evidence_file_bytes": limits.evidence_file_bytes(),
            "files_per_package": limits.files_per_package(),
            "aggregate_evidence_bytes": limits.aggregate_evidence_bytes(),
        }),
    );
    value
}

fn config_value_to_json(value: &ConfigValue) -> serde_json::Value {
    match value {
        ConfigValue::Null => serde_json::Value::Null,
        ConfigValue::Bool(value) => serde_json::Value::Bool(*value),
        ConfigValue::Integer(value) => serde_json::Value::from(*value),
        ConfigValue::String(value) => serde_json::Value::String(value.clone()),
        ConfigValue::Array(values) => {
            serde_json::Value::Array(values.iter().map(config_value_to_json).collect())
        }
        ConfigValue::Object(entries) => {
            let mut object = serde_json::Map::new();
            for (key, entry) in entries {
                object.insert(key.clone(), config_value_to_json(entry));
            }
            serde_json::Value::Object(object)
        }
    }
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
    OutputDiagnostic::new(error.code(), OutputSeverity::Error, error.to_string())
}
