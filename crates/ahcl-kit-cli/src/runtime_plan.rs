// crates/ahcl-kit-cli/src/runtime_plan.rs - Scope-specific runtime planning.
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

use super::{ConcreteRuntime, append_plan, plan_config_init, plan_project_init, plan_third_party};
use crate::{PlanRequest, PlanScope, RuntimeError, RuntimePlan};
use ahcl_kit_config::EffectiveConfig;
use ahcl_kit_core::{ChangePlan, Diagnostic, ProjectRoot, ResolvedGraph, UtcDate};
use ahcl_kit_license::VerifiedLicense;
use ahcl_kit_materials::{
    DependencyMaterialGenerator, ManagedThirdPartyInventory, ProjectFilesystem,
    ProjectGenerationPlanner, ProjectMaterialGenerator, ThirdPartyMaterialGenerator,
};

pub(super) fn plan_scoped(
    runtime: &mut ConcreteRuntime,
    filesystem: &ProjectFilesystem,
    project: &ProjectRoot,
    request: &PlanRequest<'_>,
) -> Result<RuntimePlan, RuntimeError> {
    if let Some(plan) = plan_init(runtime, filesystem, request)? {
        return Ok(plan);
    }
    plan_material(runtime, filesystem, project, request)?
        .ok_or_else(|| RuntimeError::operation("cli.plan_scope"))
}

fn plan_init(
    runtime: &ConcreteRuntime,
    filesystem: &ProjectFilesystem,
    request: &PlanRequest<'_>,
) -> Result<Option<RuntimePlan>, RuntimeError> {
    let contributors = runtime.installed.contributors();
    match request.scopes() {
        [PlanScope::ConfigInit] => {
            plan_config_init(filesystem, request.force(), contributors).map(Some)
        }
        [PlanScope::ProjectInit] => plan_project_init(filesystem, request, contributors).map(Some),
        _ => Ok(None),
    }
}

fn plan_project_files_scope(
    runtime: &mut ConcreteRuntime,
    filesystem: &ProjectFilesystem,
    project: &ProjectRoot,
    request: &PlanRequest<'_>,
) -> Result<RuntimePlan, RuntimeError> {
    let config = super::required_config(request.config())?;
    let license = super::required_license(request.license())?;
    let date = super::adoption_date(config)?;
    let mut components = runtime.plan_components(
        filesystem,
        project,
        &super::ComponentPlanInput {
            config,
            license: Some(license),
            current_date: Some(date),
            scopes: &[PlanScope::ProjectFiles],
        },
    )?;
    let mut changes = project_file_changes(filesystem, config, license, date, &mut components)?;
    append_plan(&mut changes, &components.changes)?;
    compared_runtime(
        filesystem,
        changes,
        Vec::new(),
        components.diagnostics,
        config,
    )
}

fn project_file_changes(
    filesystem: &ProjectFilesystem,
    config: &EffectiveConfig,
    license: &VerifiedLicense,
    date: UtcDate,
    components: &mut super::ComponentPlanSet,
) -> Result<ChangePlan, RuntimeError> {
    if let Some(centralized) = components.centralized_project.take() {
        return Ok(centralized);
    }
    ProjectMaterialGenerator::plan_project_files(filesystem, config, license, date)
        .map_err(super::materials_error)
}

fn plan_license_scope(
    runtime: &mut ConcreteRuntime,
    filesystem: &ProjectFilesystem,
    project: &ProjectRoot,
    request: &PlanRequest<'_>,
) -> Result<RuntimePlan, RuntimeError> {
    let config = super::required_config(request.config())?;
    let license = super::required_license(request.license())?;
    let components = runtime.plan_components(
        filesystem,
        project,
        &super::ComponentPlanInput {
            config,
            license: Some(license),
            current_date: None,
            scopes: &[PlanScope::License],
        },
    )?;
    let mut changes = license_changes(filesystem, config, license, components.centralized_project)?;
    append_plan(&mut changes, &components.changes)?;
    compared_runtime(
        filesystem,
        changes,
        Vec::new(),
        components.diagnostics,
        config,
    )
}

fn license_changes(
    filesystem: &ProjectFilesystem,
    config: &EffectiveConfig,
    license: &VerifiedLicense,
    centralized: Option<ChangePlan>,
) -> Result<ChangePlan, RuntimeError> {
    let mut changes = centralized.unwrap_or_else(ChangePlan::new);
    if changes.is_empty() {
        changes = ProjectMaterialGenerator::plan_license_sync(filesystem, config, license)
            .map_err(super::materials_error)?;
    }
    Ok(changes)
}

fn plan_dependency_scope(
    runtime: &mut ConcreteRuntime,
    filesystem: &ProjectFilesystem,
    project: &ProjectRoot,
    request: &PlanRequest<'_>,
) -> Result<RuntimePlan, RuntimeError> {
    let config = super::required_config(request.config())?;
    let root_graph = super::resolved_graph(request.adapters())?;
    let components = runtime.plan_components(
        filesystem,
        project,
        &super::ComponentPlanInput {
            config,
            license: None,
            current_date: None,
            scopes: &[PlanScope::Dependencies],
        },
    )?;
    let graph = super::merge_component_graph(&root_graph, &components.centralized_graph)?;
    let mut changes = DependencyMaterialGenerator::plan_document(filesystem, config, &graph)
        .map_err(super::materials_error)?;
    append_plan(&mut changes, &components.changes)?;
    compared_runtime(
        filesystem,
        changes,
        Vec::new(),
        components.diagnostics,
        config,
    )
}

fn plan_third_party_scope(
    runtime: &mut ConcreteRuntime,
    filesystem: &ProjectFilesystem,
    project: &ProjectRoot,
    request: &PlanRequest<'_>,
) -> Result<RuntimePlan, RuntimeError> {
    let config = super::required_config(request.config())?;
    let root_graph = super::resolved_graph(request.adapters())?;
    let components = runtime.plan_components(
        filesystem,
        project,
        &super::ComponentPlanInput {
            config,
            license: None,
            current_date: None,
            scopes: &[PlanScope::ThirdParty],
        },
    )?;
    let graph = super::merge_component_graph(&root_graph, &components.centralized_graph)?;
    let generated = plan_third_party(filesystem, config, &graph)?;
    let mut changes = generated.changes().clone();
    append_plan(&mut changes, &components.changes)?;
    let mut diagnostics = generated.diagnostics().to_vec();
    diagnostics.extend(components.diagnostics);
    Ok(RuntimePlan::new(
        changes.compare(filesystem).map_err(super::plan_error)?,
        generated.managed_removals().to_vec(),
        diagnostics,
        Some(config.materials_directory().clone()),
    ))
}

struct PreparedGenerate<'a> {
    config: &'a EffectiveConfig,
    license: &'a VerifiedLicense,
    graph: ResolvedGraph,
    components: super::ComponentPlanSet,
    date: UtcDate,
    inventory: ManagedThirdPartyInventory,
}

fn plan_project_generate(
    runtime: &mut ConcreteRuntime,
    filesystem: &ProjectFilesystem,
    project: &ProjectRoot,
    request: &PlanRequest<'_>,
) -> Result<RuntimePlan, RuntimeError> {
    let mut prepared = prepare_project_generate(runtime, filesystem, project, request)?;
    let mut plan = if prepared.components.centralized_project.is_some() {
        centralized_project_plan(filesystem, &mut prepared)?
    } else {
        generated_project_plan(filesystem, &prepared)?
    };
    attach_component_plan(filesystem, &mut plan, &mut prepared)?;
    Ok(plan)
}

fn prepare_project_generate<'a>(
    runtime: &mut ConcreteRuntime,
    filesystem: &ProjectFilesystem,
    project: &ProjectRoot,
    request: &'a PlanRequest<'a>,
) -> Result<PreparedGenerate<'a>, RuntimeError> {
    let config = super::required_config(request.config())?;
    let license = super::required_license(request.license())?;
    let root_graph = super::resolved_graph(request.adapters())?;
    let components = runtime.plan_components(
        filesystem,
        project,
        &super::ComponentPlanInput {
            config,
            license: Some(license),
            current_date: request.current_date(),
            scopes: request.scopes(),
        },
    )?;
    Ok(PreparedGenerate {
        graph: super::merge_component_graph(&root_graph, &components.centralized_graph)?,
        date: super::adoption_date(config)?,
        inventory: load_inventory(filesystem, config)?,
        config,
        license,
        components,
    })
}

fn load_inventory(
    filesystem: &ProjectFilesystem,
    config: &EffectiveConfig,
) -> Result<ManagedThirdPartyInventory, RuntimeError> {
    let managed = filesystem
        .managed_third_party_dir(config.materials_directory())
        .map_err(super::materials_error)?;
    managed.inventory().map_err(super::materials_error)
}

fn centralized_project_plan(
    filesystem: &ProjectFilesystem,
    prepared: &mut PreparedGenerate<'_>,
) -> Result<RuntimePlan, RuntimeError> {
    let Some(centralized) = prepared.components.centralized_project.take() else {
        return Err(RuntimeError::operation("cli.plan_scope"));
    };
    let dependency =
        DependencyMaterialGenerator::plan_document(filesystem, prepared.config, &prepared.graph)
            .map_err(super::materials_error)?;
    let third_party = ThirdPartyMaterialGenerator::plan_tree(
        filesystem,
        prepared.config,
        &prepared.graph,
        &prepared.inventory,
    )
    .map_err(super::materials_error)?;
    let mut changes = centralized;
    append_plan(&mut changes, &dependency)?;
    append_plan(&mut changes, third_party.changes())?;
    Ok(RuntimePlan::new(
        changes.compare(filesystem).map_err(super::plan_error)?,
        third_party.managed_removals().to_vec(),
        third_party.diagnostics().to_vec(),
        Some(prepared.config.materials_directory().clone()),
    ))
}

fn generated_project_plan(
    filesystem: &ProjectFilesystem,
    prepared: &PreparedGenerate<'_>,
) -> Result<RuntimePlan, RuntimeError> {
    let generated = ProjectGenerationPlanner::plan_all(
        filesystem,
        prepared.config,
        prepared.license,
        &prepared.graph,
        &prepared.inventory,
        prepared.date,
    )
    .map_err(super::materials_error)?;
    Ok(super::runtime_plan(
        generated,
        prepared.config.materials_directory().clone(),
    ))
}

fn attach_component_plan(
    filesystem: &ProjectFilesystem,
    plan: &mut RuntimePlan,
    prepared: &mut PreparedGenerate<'_>,
) -> Result<(), RuntimeError> {
    let mut changes = plan.changes().clone();
    append_plan(&mut changes, &prepared.components.changes)?;
    let mut diagnostics = plan.diagnostics().to_vec();
    diagnostics.extend(std::mem::take(&mut prepared.components.diagnostics));
    *plan = RuntimePlan::new(
        changes.compare(filesystem).map_err(super::plan_error)?,
        plan.managed_removals().to_vec(),
        diagnostics,
        Some(prepared.config.materials_directory().clone()),
    );
    Ok(())
}

fn compared_runtime(
    filesystem: &ProjectFilesystem,
    changes: ChangePlan,
    removals: Vec<ahcl_kit_materials::ManagedRemoval>,
    diagnostics: Vec<Diagnostic>,
    config: &EffectiveConfig,
) -> Result<RuntimePlan, RuntimeError> {
    Ok(RuntimePlan::new(
        changes.compare(filesystem).map_err(super::plan_error)?,
        removals,
        diagnostics,
        Some(config.materials_directory().clone()),
    ))
}

fn plan_material(
    runtime: &mut ConcreteRuntime,
    filesystem: &ProjectFilesystem,
    project: &ProjectRoot,
    request: &PlanRequest<'_>,
) -> Result<Option<RuntimePlan>, RuntimeError> {
    match request.scopes() {
        [PlanScope::ProjectFiles] => {
            plan_project_files_scope(runtime, filesystem, project, request).map(Some)
        }
        [PlanScope::License] => plan_license_scope(runtime, filesystem, project, request).map(Some),
        [PlanScope::Dependencies] => {
            plan_dependency_scope(runtime, filesystem, project, request).map(Some)
        }
        [PlanScope::ThirdParty] => {
            plan_third_party_scope(runtime, filesystem, project, request).map(Some)
        }
        [
            PlanScope::ProjectFiles,
            PlanScope::Dependencies,
            PlanScope::ThirdParty,
        ] => plan_project_generate(runtime, filesystem, project, request).map(Some),
        _ => Ok(None),
    }
}
