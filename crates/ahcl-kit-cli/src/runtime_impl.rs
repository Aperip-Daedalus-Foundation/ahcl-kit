// crates/ahcl-kit-cli/src/runtime_impl.rs - Concrete runtime composition for CLI commands.
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
    AdapterKind, CommandRuntime, PlanRequest, PlanScope, ResolvedAdapter, RuntimeError, RuntimePlan,
};
use ahcl_kit_cargo::{CargoAdapter, CargoResolveRequest, EvidenceLimits};
use ahcl_kit_config::{
    AhclVersion, ComponentLayout, ConfigDocument, ConfigSkeleton, EffectiveConfig, ProjectIdentity,
    ScalarValue,
};
use ahcl_kit_core::{
    ChangePlan, Diagnostic, ProjectEntry, ProjectRoot, ProjectView, RepoPath, ResolvedGraph,
    UtcDate,
};
use ahcl_kit_license::{OfficialLicenseClient, VerifiedLicense};
use ahcl_kit_materials::{
    DependencyMaterialGenerator, ManagedThirdPartyInventory, MaterialGenerationPlan, PlanApplier,
    ProjectFilesystem, ProjectGenerationPlanner, ProjectMaterialGenerator,
    ThirdPartyMaterialGenerator,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use time::OffsetDateTime;

const CONFIG_PATH: &str = ".ahclkitconfigs";

pub struct ConcreteRuntime {
    current_date: Option<UtcDate>,
    license_client: OfficialLicenseClient,
    filesystems: BTreeMap<PathBuf, ProjectFilesystem>,
}

impl ConcreteRuntime {
    pub fn new(current_date: Option<UtcDate>) -> Self {
        Self {
            current_date,
            license_client: OfficialLicenseClient::new(),
            filesystems: BTreeMap::new(),
        }
    }

    pub fn with_license_client(
        current_date: UtcDate,
        license_client: OfficialLicenseClient,
    ) -> Self {
        Self {
            current_date: Some(current_date),
            license_client,
            filesystems: BTreeMap::new(),
        }
    }

    fn plan_runtime(
        &mut self,
        project: &ProjectRoot,
        request: PlanRequest<'_>,
    ) -> Result<RuntimePlan, RuntimeError> {
        let key = project.as_path().to_path_buf();
        self.filesystems.remove(&key);
        let filesystem = ProjectFilesystem::open(project.as_path()).map_err(materials_error)?;
        let plan = match request.scopes() {
            [PlanScope::ConfigInit] => plan_config_init(&filesystem, request.force())?,
            [PlanScope::ProjectInit] => plan_project_init(&filesystem, request)?,
            [PlanScope::ProjectFiles] => {
                let config = required_config(request.config())?;
                let license = required_license(request.license())?;
                let date = adoption_date(config)?;
                let mut components = self.plan_components(
                    &filesystem,
                    project,
                    config,
                    Some(license),
                    Some(date),
                    &[PlanScope::ProjectFiles],
                )?;
                let mut changes = if let Some(centralized) = components.centralized_project.take() {
                    centralized
                } else {
                    ProjectMaterialGenerator::plan_project_files(&filesystem, config, license, date)
                        .map_err(materials_error)?
                };
                append_plan(&mut changes, &components.changes)?;
                RuntimePlan::new(
                    changes.compare(&filesystem).map_err(plan_error)?,
                    Vec::new(),
                    components.diagnostics,
                    Some(config.materials_directory().clone()),
                )
            }
            [PlanScope::License] => {
                let config = required_config(request.config())?;
                let license = required_license(request.license())?;
                let components = self.plan_components(
                    &filesystem,
                    project,
                    config,
                    Some(license),
                    None,
                    &[PlanScope::License],
                )?;
                let mut changes = components
                    .centralized_project
                    .unwrap_or_else(ChangePlan::new);
                if changes.is_empty() {
                    changes =
                        ProjectMaterialGenerator::plan_license_sync(&filesystem, config, license)
                            .map_err(materials_error)?;
                }
                append_plan(&mut changes, &components.changes)?;
                RuntimePlan::new(
                    changes.compare(&filesystem).map_err(plan_error)?,
                    Vec::new(),
                    components.diagnostics,
                    Some(config.materials_directory().clone()),
                )
            }
            [PlanScope::Dependencies] => {
                let config = required_config(request.config())?;
                let root_graph = resolved_graph(request.adapters())?;
                let components = self.plan_components(
                    &filesystem,
                    project,
                    config,
                    None,
                    None,
                    &[PlanScope::Dependencies],
                )?;
                let graph = merge_component_graph(&root_graph, &components.centralized_graph)?;
                let mut changes =
                    DependencyMaterialGenerator::plan_document(&filesystem, config, &graph)
                        .map_err(materials_error)?;
                append_plan(&mut changes, &components.changes)?;
                RuntimePlan::new(
                    changes.compare(&filesystem).map_err(plan_error)?,
                    Vec::new(),
                    components.diagnostics,
                    Some(config.materials_directory().clone()),
                )
            }
            [PlanScope::ThirdParty] => {
                let config = required_config(request.config())?;
                let root_graph = resolved_graph(request.adapters())?;
                let components = self.plan_components(
                    &filesystem,
                    project,
                    config,
                    None,
                    None,
                    &[PlanScope::ThirdParty],
                )?;
                let graph = merge_component_graph(&root_graph, &components.centralized_graph)?;
                let generated = plan_third_party(&filesystem, config, &graph)?;
                let mut changes = generated.changes().clone();
                append_plan(&mut changes, &components.changes)?;
                let mut diagnostics = generated.diagnostics().to_vec();
                diagnostics.extend(components.diagnostics);
                RuntimePlan::new(
                    changes.compare(&filesystem).map_err(plan_error)?,
                    generated.managed_removals().to_vec(),
                    diagnostics,
                    Some(config.materials_directory().clone()),
                )
            }
            [
                PlanScope::ProjectFiles,
                PlanScope::Dependencies,
                PlanScope::ThirdParty,
            ] => {
                let config = required_config(request.config())?;
                let license = required_license(request.license())?;
                let root_graph = resolved_graph(request.adapters())?;
                let mut components = self.plan_components(
                    &filesystem,
                    project,
                    config,
                    Some(license),
                    request.current_date(),
                    request.scopes(),
                )?;
                let graph = merge_component_graph(&root_graph, &components.centralized_graph)?;
                let date = adoption_date(config)?;
                let managed = filesystem
                    .managed_third_party_dir(config.materials_directory())
                    .map_err(materials_error)?;
                let inventory = managed.inventory().map_err(materials_error)?;
                let mut plan = if let Some(centralized) = components.centralized_project.take() {
                    let dependency =
                        DependencyMaterialGenerator::plan_document(&filesystem, config, &graph)
                            .map_err(materials_error)?;
                    let third_party = ThirdPartyMaterialGenerator::plan_tree(
                        &filesystem,
                        config,
                        &graph,
                        &inventory,
                    )
                    .map_err(materials_error)?;
                    let mut changes = centralized;
                    append_plan(&mut changes, &dependency)?;
                    append_plan(&mut changes, third_party.changes())?;
                    RuntimePlan::new(
                        changes.compare(&filesystem).map_err(plan_error)?,
                        third_party.managed_removals().to_vec(),
                        third_party.diagnostics().to_vec(),
                        Some(config.materials_directory().clone()),
                    )
                } else {
                    let generated = ProjectGenerationPlanner::plan_all(
                        &filesystem,
                        config,
                        license,
                        &graph,
                        &inventory,
                        date,
                    )
                    .map_err(materials_error)?;
                    runtime_plan(generated, config.materials_directory().clone())
                };
                let mut changes = plan.changes().clone();
                append_plan(&mut changes, &components.changes)?;
                let mut diagnostics = plan.diagnostics().to_vec();
                diagnostics.extend(components.diagnostics);
                plan = RuntimePlan::new(
                    changes.compare(&filesystem).map_err(plan_error)?,
                    plan.managed_removals().to_vec(),
                    diagnostics,
                    Some(config.materials_directory().clone()),
                );
                plan
            }
            _ => return Err(RuntimeError::operation("cli.plan_scope")),
        };
        self.filesystems.insert(key, filesystem);
        Ok(plan)
    }
}

impl CommandRuntime for ConcreteRuntime {
    fn load_config(&mut self, project: &ProjectRoot) -> Result<EffectiveConfig, RuntimeError> {
        load_config(project)
    }

    fn prepare_project_init(
        &mut self,
        project: &ProjectRoot,
        identity: Option<&ProjectIdentity>,
        current_date: UtcDate,
    ) -> Result<EffectiveConfig, RuntimeError> {
        let filesystem = ProjectFilesystem::open(project.as_path()).map_err(materials_error)?;
        match config_entry(&filesystem)? {
            ProjectEntry::Absent => prepared_config(
                identity.ok_or_else(|| RuntimeError::operation("config.identity_required"))?,
                AhclVersion::V1_2,
                current_date,
            ),
            ProjectEntry::File(bytes) => {
                let existing = parse_config(&bytes)?;
                if complete_identity(&existing) {
                    Ok(existing)
                } else {
                    prepared_config(
                        identity
                            .ok_or_else(|| RuntimeError::operation("config.identity_required"))?,
                        existing.license().version(),
                        current_date,
                    )
                }
            }
            ProjectEntry::Other => Err(RuntimeError::operation("config.invalid")),
        }
    }

    fn current_utc_date(&mut self) -> Result<UtcDate, RuntimeError> {
        self.current_date
            .ok_or_else(|| RuntimeError::operation("cli.current_date"))
    }

    fn fetch_official_license(
        &mut self,
        version: AhclVersion,
    ) -> Result<VerifiedLicense, RuntimeError> {
        self.license_client
            .fetch(version)
            .map_err(|error| RuntimeError::with_source(error.code().as_str(), error))
    }

    fn resolve_adapter(
        &mut self,
        adapter: AdapterKind,
        project: &ProjectRoot,
        config: &EffectiveConfig,
    ) -> Result<ResolvedGraph, RuntimeError> {
        if adapter != AdapterKind::CARGO {
            return Err(RuntimeError::operation("cli.adapter_unsupported"));
        }
        let limits = config.limits();
        let request = CargoResolveRequest::from_config(
            project.clone(),
            config,
            config.generation().strict_license_files(),
        )
        .with_limits(EvidenceLimits::new(
            limits.evidence_file_bytes(),
            limits.files_per_package(),
            limits.aggregate_evidence_bytes(),
        ));
        CargoAdapter::new()
            .resolve_request(&request)
            .map_err(|error| RuntimeError::with_source(error.code(), error))
    }

    fn plan(
        &mut self,
        project: &ProjectRoot,
        request: PlanRequest<'_>,
    ) -> Result<ChangePlan, RuntimeError> {
        let plan = self.plan_runtime(project, request)?;
        if !plan.managed_removals().is_empty() {
            return Err(RuntimeError::operation("cli.managed_plan_required"));
        }
        Ok(plan.changes().clone())
    }

    fn compare(&mut self, _: &ProjectRoot, plan: &ChangePlan) -> Result<ChangePlan, RuntimeError> {
        Ok(plan.clone())
    }

    fn apply(&mut self, project: &ProjectRoot, plan: &ChangePlan) -> Result<(), RuntimeError> {
        let filesystem = self.filesystem(project)?;
        PlanApplier::apply(filesystem, plan).map_err(materials_error)
    }

    fn plan_project(
        &mut self,
        project: &ProjectRoot,
        request: PlanRequest<'_>,
    ) -> Result<RuntimePlan, RuntimeError> {
        self.plan_runtime(project, request)
    }

    fn compare_project(
        &mut self,
        _: &ProjectRoot,
        plan: RuntimePlan,
    ) -> Result<RuntimePlan, RuntimeError> {
        Ok(plan)
    }

    fn apply_project(
        &mut self,
        project: &ProjectRoot,
        plan: &RuntimePlan,
    ) -> Result<(), RuntimeError> {
        let filesystem = self.filesystem(project)?;
        if let Some(materials_directory) = plan.managed_materials_directory() {
            let managed = filesystem
                .managed_third_party_dir(materials_directory)
                .map_err(materials_error)?;
            PlanApplier::apply_managed_removals(&managed, plan.managed_removals())
                .map_err(materials_error)?;
        }
        PlanApplier::apply(filesystem, plan.changes()).map_err(materials_error)
    }
}

impl ConcreteRuntime {
    fn license_for_component(
        &mut self,
        fallback: Option<&VerifiedLicense>,
        config: &EffectiveConfig,
    ) -> Result<VerifiedLicense, RuntimeError> {
        let version = config.license().version();
        if let Some(license) = fallback.filter(|license| license.version == version) {
            return Ok(license.clone());
        }
        self.license_client
            .fetch(version)
            .map_err(|error| RuntimeError::with_source(error.code().as_str(), error))
    }

    fn plan_components(
        &mut self,
        root_view: &ProjectFilesystem,
        project: &ProjectRoot,
        config: &EffectiveConfig,
        license: Option<&VerifiedLicense>,
        current_date: Option<UtcDate>,
        scopes: &[PlanScope],
    ) -> Result<ComponentPlanSet, RuntimeError> {
        let wants_project = scopes.contains(&PlanScope::ProjectFiles);
        let wants_license = scopes.contains(&PlanScope::License);
        let wants_dependencies = scopes.contains(&PlanScope::Dependencies);
        let wants_third_party = scopes.contains(&PlanScope::ThirdParty);
        let cargo = CargoAdapter::new();
        let mut result = ComponentPlanSet::default();
        let mut centralized_configs = Vec::new();

        for component in config.rust().cargo().components() {
            if !component.enabled() {
                continue;
            }
            let resolution = cargo
                .resolve_component(project, config, component)
                .map_err(|error| RuntimeError::with_source(error.code(), error))?;
            if component.layout() == ComponentLayout::Centralized {
                merge_graph(&mut result.centralized_graph, resolution.graph())?;
                centralized_configs.push(resolution.config().clone());
                continue;
            }

            let component_root = resolution.component_root().cloned();
            let component_path = component_root
                .as_ref()
                .map(|path| project.resolve(path))
                .unwrap_or_else(|| project.as_path().to_path_buf());
            let component_view =
                ProjectFilesystem::open(&component_path).map_err(materials_error)?;
            let component_config = resolution.config();
            let component_license = if wants_project || wants_license {
                Some(self.license_for_component(license, component_config)?)
            } else {
                None
            };
            let mut local = ChangePlan::new();

            if wants_project {
                let license = required_license(component_license.as_ref())?;
                let date = component_config
                    .project()
                    .adoption_date()
                    .or(current_date)
                    .ok_or_else(|| RuntimeError::operation("config.adoption_date_required"))?;
                let generated = ProjectMaterialGenerator::plan_project_files(
                    &component_view,
                    component_config,
                    license,
                    date,
                )
                .map_err(materials_error)?;
                append_plan(&mut local, &generated)?;
            }
            if wants_license {
                let license = required_license(component_license.as_ref())?;
                let generated = ProjectMaterialGenerator::plan_license_sync(
                    &component_view,
                    component_config,
                    license,
                )
                .map_err(materials_error)?;
                append_plan(&mut local, &generated)?;
            }
            if wants_dependencies {
                let generated = DependencyMaterialGenerator::plan_document(
                    &component_view,
                    component_config,
                    resolution.graph(),
                )
                .map_err(materials_error)?;
                append_plan(&mut local, &generated)?;
            }
            if wants_third_party {
                let managed = component_view
                    .managed_third_party_dir(component_config.materials_directory())
                    .map_err(materials_error)?;
                let inventory = managed.inventory().map_err(materials_error)?;
                let generated = ThirdPartyMaterialGenerator::plan_tree(
                    &component_view,
                    component_config,
                    resolution.graph(),
                    &inventory,
                )
                .map_err(materials_error)?;
                for diagnostic in generated.diagnostics() {
                    result
                        .diagnostics
                        .push(prefix_diagnostic(diagnostic, component_root.as_ref())?);
                }
                append_plan(&mut local, generated.changes())?;
            }

            let prefixed = prefix_plan(&local, component_root.as_ref())?;
            append_plan(&mut result.changes, &prefixed)?;
        }

        if !centralized_configs.is_empty() && wants_project {
            let license = required_license(license)?;
            let date = config
                .project()
                .adoption_date()
                .or(current_date)
                .ok_or_else(|| RuntimeError::operation("config.adoption_date_required"))?;
            let mut scopes = Vec::with_capacity(centralized_configs.len() + 1);
            scopes.push(config.clone());
            scopes.extend(centralized_configs.iter().cloned());
            result.centralized_project = Some(
                ProjectMaterialGenerator::plan_centralized_files(root_view, &scopes, license, date)
                    .map_err(materials_error)?,
            );
        }

        if !centralized_configs.is_empty() && wants_license {
            let sync_config = if config.license().enabled() {
                config.clone()
            } else {
                centralized_configs
                    .first()
                    .cloned()
                    .ok_or_else(|| RuntimeError::operation("license.unavailable"))?
            };
            let license = required_license(license)?;
            let generated =
                ProjectMaterialGenerator::plan_license_sync(root_view, &sync_config, license)
                    .map_err(materials_error)?;
            result.centralized_project = Some(generated);
        }

        Ok(result)
    }

    fn filesystem(&self, project: &ProjectRoot) -> Result<&ProjectFilesystem, RuntimeError> {
        self.filesystems
            .get(project.as_path())
            .ok_or_else(|| RuntimeError::operation("cli.plan_capability_missing"))
    }
}

#[derive(Default)]
struct ComponentPlanSet {
    centralized_graph: ResolvedGraph,
    centralized_project: Option<ChangePlan>,
    changes: ChangePlan,
    diagnostics: Vec<Diagnostic>,
}

fn merge_graph(target: &mut ResolvedGraph, source: &ResolvedGraph) -> Result<(), RuntimeError> {
    for package in &source.packages {
        if let Some(existing) = target.packages.iter().find(|item| item.id == package.id) {
            if existing != package {
                return Err(RuntimeError::operation("cargo.component_package_conflict"));
            }
        } else {
            target.packages.push(package.clone());
        }
    }
    for edge in &source.edges {
        if !target.edges.contains(edge) {
            target.edges.push(edge.clone());
        }
    }
    target
        .packages
        .sort_by(|left, right| left.id.cmp(&right.id));
    target.edges.sort_by(|left, right| {
        left.from_package_id
            .cmp(&right.from_package_id)
            .then_with(|| left.to_package_id.cmp(&right.to_package_id))
            .then_with(|| dependency_kind_rank(left.kind).cmp(&dependency_kind_rank(right.kind)))
            .then_with(|| left.target_conditions.cmp(&right.target_conditions))
            .then_with(|| left.direct.cmp(&right.direct))
    });
    Ok(())
}

fn merge_component_graph(
    root: &ResolvedGraph,
    centralized: &ResolvedGraph,
) -> Result<ResolvedGraph, RuntimeError> {
    let mut merged = root.clone();
    merge_graph(&mut merged, centralized)?;
    Ok(merged)
}

fn dependency_kind_rank(kind: ahcl_kit_core::DependencyKind) -> u8 {
    match kind {
        ahcl_kit_core::DependencyKind::Normal => 0,
        ahcl_kit_core::DependencyKind::Build => 1,
        ahcl_kit_core::DependencyKind::Development => 2,
    }
}

fn prefix_plan(source: &ChangePlan, prefix: Option<&RepoPath>) -> Result<ChangePlan, RuntimeError> {
    let mut result = ChangePlan::new();
    for change in source.changes() {
        let path = prefixed_path(prefix, change.path())?;
        match change.kind() {
            ahcl_kit_core::ChangeKind::Create | ahcl_kit_core::ChangeKind::Replace => {
                let bytes = change
                    .bytes()
                    .ok_or_else(|| RuntimeError::operation("materials.plan_missing_bytes"))?;
                result.write(path, bytes.to_vec()).map_err(plan_error)?;
            }
            ahcl_kit_core::ChangeKind::Remove => result.remove(path).map_err(plan_error)?,
        }
    }
    Ok(result)
}

fn prefixed_path(prefix: Option<&RepoPath>, path: &RepoPath) -> Result<RepoPath, RuntimeError> {
    match prefix {
        None => Ok(path.clone()),
        Some(prefix) => RepoPath::parse(format!("{prefix}/{path}"))
            .map_err(|_| RuntimeError::operation("materials.path_invalid")),
    }
}

fn prefix_diagnostic(
    diagnostic: &Diagnostic,
    prefix: Option<&RepoPath>,
) -> Result<Diagnostic, RuntimeError> {
    let mut result = diagnostic.clone();
    if let Some(path) = diagnostic.path.as_ref() {
        result.path = Some(prefixed_path(prefix, path)?);
    }
    Ok(result)
}

fn append_plan(target: &mut ChangePlan, source: &ChangePlan) -> Result<(), RuntimeError> {
    for change in source.changes() {
        if let Some(existing) = target
            .changes()
            .into_iter()
            .find(|existing| existing.path() == change.path())
        {
            if existing.kind() == change.kind() && existing.bytes() == change.bytes() {
                continue;
            }
            return Err(RuntimeError::operation("materials.plan_conflict"));
        }
        match change.kind() {
            ahcl_kit_core::ChangeKind::Create | ahcl_kit_core::ChangeKind::Replace => {
                let bytes = change
                    .bytes()
                    .ok_or_else(|| RuntimeError::operation("materials.plan_missing_bytes"))?;
                target
                    .write(change.path().clone(), bytes.to_vec())
                    .map_err(plan_error)?;
            }
            ahcl_kit_core::ChangeKind::Remove => {
                target.remove(change.path().clone()).map_err(plan_error)?;
            }
        }
    }
    Ok(())
}

pub fn system_utc_date() -> Result<UtcDate, RuntimeError> {
    let now = OffsetDateTime::now_utc();
    let year = u16::try_from(now.year())
        .map_err(|error| RuntimeError::with_source("cli.current_date", error))?;
    UtcDate::new(year, u8::from(now.month()), now.day())
        .map_err(|error| RuntimeError::with_source("cli.current_date", error))
}

fn load_config(project: &ProjectRoot) -> Result<EffectiveConfig, RuntimeError> {
    let filesystem = ProjectFilesystem::open(project.as_path()).map_err(materials_error)?;
    match config_entry(&filesystem)? {
        ProjectEntry::File(bytes) => parse_config(&bytes),
        ProjectEntry::Absent => Err(RuntimeError::operation("config.not_found")),
        ProjectEntry::Other => Err(RuntimeError::operation("config.invalid")),
    }
}

fn config_entry(filesystem: &ProjectFilesystem) -> Result<ProjectEntry, RuntimeError> {
    let path = RepoPath::parse(CONFIG_PATH)
        .map_err(|error| RuntimeError::with_source("config.path", error))?;
    filesystem
        .entry(&path)
        .map_err(|error| RuntimeError::with_source("config.read", error))
}

fn parse_config(bytes: &[u8]) -> Result<EffectiveConfig, RuntimeError> {
    let document = ConfigDocument::parse_bytes(bytes)
        .map_err(|error| RuntimeError::with_source("config.invalid", error))?;
    EffectiveConfig::resolve(&document)
        .map_err(|error| RuntimeError::with_source("config.invalid", error))
}

fn prepared_config(
    identity: &ProjectIdentity,
    version: AhclVersion,
    current_date: UtcDate,
) -> Result<EffectiveConfig, RuntimeError> {
    let mut document = ConfigDocument::parse(&ConfigSkeleton::render_populated(identity))
        .map_err(|error| RuntimeError::with_source("config.invalid", error))?;
    let (version, directory) = match version {
        AhclVersion::V1_0 => ("1.0", "AHCL"),
        AhclVersion::V1_1 => ("1.1", ".ahcl"),
        AhclVersion::V1_2 => ("1.2", ".ahcl"),
    };
    for (section, key, value) in [
        (None, "materials-directory", directory.to_owned()),
        (Some("license"), "version", version.to_owned()),
        (Some("project"), "adoption-date", current_date.to_string()),
    ] {
        document
            .upsert_scalar(section, key, ScalarValue::String(value))
            .map_err(|error| RuntimeError::with_source("config.invalid", error))?;
    }
    EffectiveConfig::resolve(&document)
        .map_err(|error| RuntimeError::with_source("config.invalid", error))
}

fn plan_config_init(
    filesystem: &ProjectFilesystem,
    force: bool,
) -> Result<RuntimePlan, RuntimeError> {
    let path = RepoPath::parse(CONFIG_PATH)
        .map_err(|error| RuntimeError::with_source("config.path", error))?;
    let entry = filesystem
        .entry(&path)
        .map_err(|error| RuntimeError::with_source("config.read", error))?;
    if !force && entry != ProjectEntry::Absent {
        return Err(RuntimeError::operation("config.exists"));
    }
    let mut desired = ChangePlan::new();
    desired
        .write(path, ConfigSkeleton::render().as_bytes().to_vec())
        .map_err(|error| RuntimeError::with_source("materials.plan", error))?;
    let compared = desired
        .compare(filesystem)
        .map_err(|error| RuntimeError::with_source("materials.plan", error))?;
    Ok(RuntimePlan::from_changes(compared))
}

fn plan_project_init(
    filesystem: &ProjectFilesystem,
    request: PlanRequest<'_>,
) -> Result<RuntimePlan, RuntimeError> {
    let license = required_license(request.license())?;
    let current_date = request
        .current_date()
        .ok_or_else(|| RuntimeError::operation("cli.current_date"))?;
    let entry = config_entry(filesystem)?;
    let (identity, force) = match entry {
        ProjectEntry::Absent => (request.identity(), false),
        ProjectEntry::File(bytes) => {
            let config = parse_config(&bytes)?;
            if complete_identity(&config) {
                (None, false)
            } else {
                (request.identity(), true)
            }
        }
        ProjectEntry::Other => return Err(RuntimeError::operation("config.invalid")),
    };
    let changes =
        ProjectMaterialGenerator::plan_init(filesystem, identity, license, current_date, force)
            .map_err(materials_error)?;
    Ok(RuntimePlan::from_changes(changes))
}

fn plan_third_party(
    filesystem: &ProjectFilesystem,
    config: &EffectiveConfig,
    graph: &ResolvedGraph,
) -> Result<MaterialGenerationPlan, RuntimeError> {
    let managed = filesystem
        .managed_third_party_dir(config.materials_directory())
        .map_err(materials_error)?;
    let inventory: ManagedThirdPartyInventory = managed.inventory().map_err(materials_error)?;
    ThirdPartyMaterialGenerator::plan_tree(filesystem, config, graph, &inventory)
        .map_err(materials_error)
}

fn runtime_plan(generated: MaterialGenerationPlan, materials_directory: RepoPath) -> RuntimePlan {
    RuntimePlan::new(
        generated.changes().clone(),
        generated.managed_removals().to_vec(),
        generated.diagnostics().to_vec(),
        Some(materials_directory),
    )
}

fn resolved_graph(adapters: &[ResolvedAdapter]) -> Result<ResolvedGraph, RuntimeError> {
    ResolvedAdapter::merge_all(adapters)
}

fn required_config(config: Option<&EffectiveConfig>) -> Result<&EffectiveConfig, RuntimeError> {
    config.ok_or_else(|| RuntimeError::operation("config.unavailable"))
}

fn required_license(license: Option<&VerifiedLicense>) -> Result<&VerifiedLicense, RuntimeError> {
    license.ok_or_else(|| RuntimeError::operation("license.unavailable"))
}

fn adoption_date(config: &EffectiveConfig) -> Result<UtcDate, RuntimeError> {
    config
        .project()
        .adoption_date()
        .ok_or_else(|| RuntimeError::operation("config.adoption_date_required"))
}

fn complete_identity(config: &EffectiveConfig) -> bool {
    !config.project().name().trim().is_empty()
        && !config.project().canonical_repository().trim().is_empty()
        && !config.project().right_holders().is_empty()
        && config
            .project()
            .right_holders()
            .iter()
            .all(|holder| !holder.trim().is_empty())
}

fn materials_error(error: ahcl_kit_materials::MaterialsError) -> RuntimeError {
    RuntimeError::with_source(error.code().as_str(), error)
}

fn plan_error(error: ahcl_kit_core::PlanError) -> RuntimeError {
    RuntimeError::with_source("materials.plan", error)
}
