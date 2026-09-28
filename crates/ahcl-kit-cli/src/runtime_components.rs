// crates/ahcl-kit-cli/src/runtime_components.rs - Component-scoped material planning.
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
    ComponentPlanInput, ComponentPlanSet, ConcreteRuntime, RuntimeError, adoption_date_or_current,
};
use crate::PlanScope;
use ahcl_kit_config::{ComponentResolution, EffectiveConfig, LanguageHost};
use ahcl_kit_core::{ChangePlan, ProjectRoot, RepoPath, ResolvedGraph};
use ahcl_kit_license::VerifiedLicense;
use ahcl_kit_materials::{
    DependencyMaterialGenerator, MaterialGenerationPlan, ProjectFilesystem,
    ProjectMaterialGenerator, ThirdPartyMaterialGenerator,
};
use std::path::PathBuf;

struct ScopeWants {
    project: bool,
    license: bool,
    dependencies: bool,
    third_party: bool,
}

struct ComponentAccumulation {
    result: ComponentPlanSet,
    centralized_configs: Vec<EffectiveConfig>,
}

struct OpenedComponent<'a> {
    root: Option<RepoPath>,
    view: ProjectFilesystem,
    config: &'a EffectiveConfig,
}

pub(super) fn plan(
    runtime: &mut ConcreteRuntime,
    root_view: &ProjectFilesystem,
    project: &ProjectRoot,
    input: &ComponentPlanInput<'_>,
) -> Result<ComponentPlanSet, RuntimeError> {
    let wants = scope_wants(input.scopes);
    let mut accumulated = ComponentAccumulation {
        result: ComponentPlanSet::default(),
        centralized_configs: Vec::new(),
    };
    plan_hosts(runtime, project, input, &wants, &mut accumulated)?;
    plan_centralized(root_view, input, &wants, &mut accumulated)?;
    Ok(accumulated.result)
}

fn scope_wants(scopes: &[PlanScope]) -> ScopeWants {
    ScopeWants {
        project: scopes.contains(&PlanScope::ProjectFiles),
        license: scopes.contains(&PlanScope::License),
        dependencies: scopes.contains(&PlanScope::Dependencies),
        third_party: scopes.contains(&PlanScope::ThirdParty),
    }
}

fn plan_hosts(
    runtime: &mut ConcreteRuntime,
    project: &ProjectRoot,
    input: &ComponentPlanInput<'_>,
    wants: &ScopeWants,
    accumulated: &mut ComponentAccumulation,
) -> Result<(), RuntimeError> {
    // Hosts are static registrations, so this copy does not borrow the runtime.
    let hosts = runtime.installed.hosts();
    for host in hosts {
        plan_host(runtime, *host, project, input, wants, accumulated)?;
    }
    Ok(())
}

fn plan_host(
    runtime: &mut ConcreteRuntime,
    host: &dyn LanguageHost,
    project: &ProjectRoot,
    input: &ComponentPlanInput<'_>,
    wants: &ScopeWants,
    accumulated: &mut ComponentAccumulation,
) -> Result<(), RuntimeError> {
    let components = host
        .resolve_components(project, input.config)
        .map_err(|error| RuntimeError::with_source(error.code(), error))?;
    for resolution in components {
        plan_resolution(runtime, project, input, wants, accumulated, &resolution)?;
    }
    Ok(())
}

fn plan_resolution(
    runtime: &mut ConcreteRuntime,
    project: &ProjectRoot,
    input: &ComponentPlanInput<'_>,
    wants: &ScopeWants,
    accumulated: &mut ComponentAccumulation,
    resolution: &ComponentResolution,
) -> Result<(), RuntimeError> {
    if resolution.centralized() {
        return record_centralized(accumulated, resolution);
    }
    plan_local(runtime, project, input, wants, accumulated, resolution)
}

fn record_centralized(
    accumulated: &mut ComponentAccumulation,
    resolution: &ComponentResolution,
) -> Result<(), RuntimeError> {
    super::merge_graph(
        &mut accumulated.result.centralized_graph,
        resolution.graph(),
    )?;
    accumulated
        .centralized_configs
        .push(resolution.config().clone());
    Ok(())
}

fn plan_local(
    runtime: &mut ConcreteRuntime,
    project: &ProjectRoot,
    input: &ComponentPlanInput<'_>,
    wants: &ScopeWants,
    accumulated: &mut ComponentAccumulation,
    resolution: &ComponentResolution,
) -> Result<(), RuntimeError> {
    let opened = open_component(project, resolution)?;
    let license = component_license(runtime, input, wants, opened.config)?;
    let mut local = ChangePlan::new();
    append_project_scope(&opened, input, wants, license.as_ref(), &mut local)?;
    append_license_scope(&opened, wants, license.as_ref(), &mut local)?;
    append_dependency_scope(&opened, resolution.graph(), wants, &mut local)?;
    append_third_party_scope(&opened, resolution.graph(), wants, accumulated, &mut local)?;
    let prefixed = super::prefix_plan(&local, opened.root.as_ref())?;
    super::append_plan(&mut accumulated.result.changes, &prefixed)
}

fn open_component<'a>(
    project: &ProjectRoot,
    resolution: &'a ComponentResolution,
) -> Result<OpenedComponent<'a>, RuntimeError> {
    let root = resolution.component_root().cloned();
    let path = component_path(project, root.as_ref());
    let view = ProjectFilesystem::open(&path).map_err(super::materials_error)?;
    Ok(OpenedComponent {
        root,
        view,
        config: resolution.config(),
    })
}

fn component_path(project: &ProjectRoot, root: Option<&RepoPath>) -> PathBuf {
    match root {
        Some(path) => project.resolve(path),
        None => project.as_path().to_path_buf(),
    }
}

fn component_license(
    runtime: &mut ConcreteRuntime,
    input: &ComponentPlanInput<'_>,
    wants: &ScopeWants,
    config: &EffectiveConfig,
) -> Result<Option<VerifiedLicense>, RuntimeError> {
    if !needs_component_license(wants) {
        return Ok(None);
    }
    runtime
        .license_for_component(input.license, config)
        .map(Some)
}

fn needs_component_license(wants: &ScopeWants) -> bool {
    wants.project || wants.license
}

fn append_project_scope(
    opened: &OpenedComponent<'_>,
    input: &ComponentPlanInput<'_>,
    wants: &ScopeWants,
    license: Option<&VerifiedLicense>,
    local: &mut ChangePlan,
) -> Result<(), RuntimeError> {
    if !wants.project {
        return Ok(());
    }
    let license = super::required_license(license)?;
    let date = adoption_date_or_current(opened.config, input.current_date)?;
    let generated =
        ProjectMaterialGenerator::plan_project_files(&opened.view, opened.config, license, date)
            .map_err(super::materials_error)?;
    super::append_plan(local, &generated)
}

fn append_license_scope(
    opened: &OpenedComponent<'_>,
    wants: &ScopeWants,
    license: Option<&VerifiedLicense>,
    local: &mut ChangePlan,
) -> Result<(), RuntimeError> {
    if !wants.license {
        return Ok(());
    }
    let license = super::required_license(license)?;
    let generated =
        ProjectMaterialGenerator::plan_license_sync(&opened.view, opened.config, license)
            .map_err(super::materials_error)?;
    super::append_plan(local, &generated)
}

fn append_dependency_scope(
    opened: &OpenedComponent<'_>,
    graph: &ResolvedGraph,
    wants: &ScopeWants,
    local: &mut ChangePlan,
) -> Result<(), RuntimeError> {
    if !wants.dependencies {
        return Ok(());
    }
    let generated = DependencyMaterialGenerator::plan_document(&opened.view, opened.config, graph)
        .map_err(super::materials_error)?;
    super::append_plan(local, &generated)
}

fn append_third_party_scope(
    opened: &OpenedComponent<'_>,
    graph: &ResolvedGraph,
    wants: &ScopeWants,
    accumulated: &mut ComponentAccumulation,
    local: &mut ChangePlan,
) -> Result<(), RuntimeError> {
    if !wants.third_party {
        return Ok(());
    }
    let generated = third_party_plan(&opened.view, opened.config, graph)?;
    push_diagnostics(&generated, opened.root.as_ref(), accumulated)?;
    super::append_plan(local, generated.changes())
}

fn third_party_plan(
    view: &ProjectFilesystem,
    config: &EffectiveConfig,
    graph: &ResolvedGraph,
) -> Result<MaterialGenerationPlan, RuntimeError> {
    let managed = view
        .managed_third_party_dir(config.materials_directory())
        .map_err(super::materials_error)?;
    let inventory = managed.inventory().map_err(super::materials_error)?;
    ThirdPartyMaterialGenerator::plan_tree(view, config, graph, &inventory)
        .map_err(super::materials_error)
}

fn push_diagnostics(
    generated: &MaterialGenerationPlan,
    root: Option<&RepoPath>,
    accumulated: &mut ComponentAccumulation,
) -> Result<(), RuntimeError> {
    for diagnostic in generated.diagnostics() {
        accumulated
            .result
            .diagnostics
            .push(super::prefix_diagnostic(diagnostic, root)?);
    }
    Ok(())
}

fn plan_centralized(
    root_view: &ProjectFilesystem,
    input: &ComponentPlanInput<'_>,
    wants: &ScopeWants,
    accumulated: &mut ComponentAccumulation,
) -> Result<(), RuntimeError> {
    if accumulated.centralized_configs.is_empty() {
        return Ok(());
    }
    // License sync replaces an earlier centralized project plan. Callers keep
    // one `centralized_project` slot, so the two scopes are not merged here.
    if wants.project {
        accumulated.result.centralized_project =
            Some(centralized_project_plan(root_view, input, accumulated)?);
    }
    if wants.license {
        accumulated.result.centralized_project =
            Some(centralized_license_plan(root_view, input, accumulated)?);
    }
    Ok(())
}

fn centralized_project_plan(
    root_view: &ProjectFilesystem,
    input: &ComponentPlanInput<'_>,
    accumulated: &ComponentAccumulation,
) -> Result<ChangePlan, RuntimeError> {
    let license = super::required_license(input.license)?;
    let date = adoption_date_or_current(input.config, input.current_date)?;
    let scopes = centralized_scopes(input.config, &accumulated.centralized_configs);
    ProjectMaterialGenerator::plan_centralized_files(root_view, &scopes, license, date)
        .map_err(super::materials_error)
}

fn centralized_scopes(
    config: &EffectiveConfig,
    centralized: &[EffectiveConfig],
) -> Vec<EffectiveConfig> {
    let mut scopes = Vec::with_capacity(centralized.len() + 1);
    scopes.push(config.clone());
    scopes.extend(centralized.iter().cloned());
    scopes
}

fn centralized_license_plan(
    root_view: &ProjectFilesystem,
    input: &ComponentPlanInput<'_>,
    accumulated: &ComponentAccumulation,
) -> Result<ChangePlan, RuntimeError> {
    let sync_config = license_sync_config(input.config, &accumulated.centralized_configs)?;
    let license = super::required_license(input.license)?;
    ProjectMaterialGenerator::plan_license_sync(root_view, &sync_config, license)
        .map_err(super::materials_error)
}

fn license_sync_config(
    config: &EffectiveConfig,
    centralized: &[EffectiveConfig],
) -> Result<EffectiveConfig, RuntimeError> {
    if config.license().enabled() {
        return Ok(config.clone());
    }
    centralized
        .first()
        .cloned()
        .ok_or_else(|| RuntimeError::operation("license.unavailable"))
}
