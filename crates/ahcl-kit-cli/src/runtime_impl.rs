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
    AhclVersion, ConfigDocument, ConfigSkeleton, EffectiveConfig, ProjectIdentity, ScalarValue,
};
use ahcl_kit_core::{
    ChangePlan, ProjectEntry, ProjectRoot, ProjectView, RepoPath, ResolvedGraph, UtcDate,
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
                RuntimePlan::from_changes(
                    ProjectMaterialGenerator::plan_project_files(
                        &filesystem,
                        config,
                        license,
                        date,
                    )
                    .map_err(materials_error)?,
                )
            }
            [PlanScope::License] => {
                let config = required_config(request.config())?;
                let license = required_license(request.license())?;
                RuntimePlan::from_changes(
                    ProjectMaterialGenerator::plan_license_sync(&filesystem, config, license)
                        .map_err(materials_error)?,
                )
            }
            [PlanScope::Dependencies] => {
                let config = required_config(request.config())?;
                let graph = resolved_graph(request.adapters())?;
                RuntimePlan::from_changes(
                    DependencyMaterialGenerator::plan_document(&filesystem, config, &graph)
                        .map_err(materials_error)?,
                )
            }
            [PlanScope::ThirdParty] => {
                let config = required_config(request.config())?;
                let graph = resolved_graph(request.adapters())?;
                let generated = plan_third_party(&filesystem, config, &graph)?;
                runtime_plan(generated, config.materials_directory().clone())
            }
            [
                PlanScope::ProjectFiles,
                PlanScope::Dependencies,
                PlanScope::ThirdParty,
            ] => {
                let config = required_config(request.config())?;
                let license = required_license(request.license())?;
                let graph = resolved_graph(request.adapters())?;
                let date = adoption_date(config)?;
                let managed = filesystem
                    .managed_third_party_dir(config.materials_directory())
                    .map_err(materials_error)?;
                let inventory = managed.inventory().map_err(materials_error)?;
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
                AhclVersion::V1_1,
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
    fn filesystem(&self, project: &ProjectRoot) -> Result<&ProjectFilesystem, RuntimeError> {
        self.filesystems
            .get(project.as_path())
            .ok_or_else(|| RuntimeError::operation("cli.plan_capability_missing"))
    }
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
