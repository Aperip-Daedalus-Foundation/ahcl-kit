// crates/ahcl-kit-cli/src/runtime_impl.rs - Concrete runtime composition for CLI commands.
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
    AdapterKind, CommandRuntime, PlanRequest, PlanScope, ResolvedAdapter, RuntimeError, RuntimePlan,
};
use ahcl_kit_config::{
    AhclVersion, ConfigDocument, ConfigSkeleton, EffectiveConfig, LanguageContributor,
    LanguageInstallation, ProjectIdentity, ScalarValue,
};
use ahcl_kit_core::{
    ChangePlan, Diagnostic, ProjectEntry, ProjectRoot, ProjectView, RepoPath, ResolvedGraph,
    UtcDate,
};
use ahcl_kit_license::{OfficialLicenseClient, VerifiedLicense};
use ahcl_kit_materials::{
    ManagedThirdPartyInventory, MaterialGenerationPlan, PlanApplier, ProjectFilesystem,
    ProjectMaterialGenerator, ThirdPartyMaterialGenerator,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use time::OffsetDateTime;

#[path = "runtime_components.rs"]
mod runtime_components;
#[path = "runtime_plan.rs"]
mod runtime_plan;

const CONFIG_PATH: &str = ".ahclkitconfigs";

struct ComponentPlanInput<'a> {
    config: &'a EffectiveConfig,
    license: Option<&'a VerifiedLicense>,
    current_date: Option<UtcDate>,
    scopes: &'a [PlanScope],
}

pub struct ConcreteRuntime {
    current_date: Option<UtcDate>,
    license_client: OfficialLicenseClient,
    filesystems: BTreeMap<PathBuf, ProjectFilesystem>,
    installed: LanguageInstallation,
}

impl ConcreteRuntime {
    pub fn new(current_date: Option<UtcDate>, installed: LanguageInstallation) -> Self {
        Self {
            current_date,
            license_client: OfficialLicenseClient::new(),
            filesystems: BTreeMap::new(),
            installed,
        }
    }

    pub fn with_license_client(
        current_date: UtcDate,
        license_client: OfficialLicenseClient,
        installed: LanguageInstallation,
    ) -> Self {
        Self {
            current_date: Some(current_date),
            license_client,
            filesystems: BTreeMap::new(),
            installed,
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
        let plan = runtime_plan::plan_scoped(self, &filesystem, project, &request)?;
        self.filesystems.insert(key, filesystem);
        Ok(plan)
    }
}

impl CommandRuntime for ConcreteRuntime {
    fn ecosystems(&self) -> LanguageInstallation {
        self.installed
    }

    fn load_config(&mut self, project: &ProjectRoot) -> Result<EffectiveConfig, RuntimeError> {
        load_config(project, self.installed.contributors())
    }

    fn prepare_project_init(
        &mut self,
        project: &ProjectRoot,
        identity: Option<&ProjectIdentity>,
        current_date: UtcDate,
    ) -> Result<EffectiveConfig, RuntimeError> {
        let filesystem = ProjectFilesystem::open(project.as_path()).map_err(materials_error)?;
        prepare_opened_init(
            &filesystem,
            identity,
            current_date,
            self.installed.contributors(),
        )
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
        let host = self
            .installed
            .hosts()
            .iter()
            .copied()
            .find(|host| host.language_id() == adapter.name())
            .ok_or_else(|| RuntimeError::operation("cli.adapter_unsupported"))?;
        host.resolve(project, config)
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
        input: &ComponentPlanInput<'_>,
    ) -> Result<ComponentPlanSet, RuntimeError> {
        runtime_components::plan(self, root_view, project, input)
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
                return Err(RuntimeError::operation("component.package_conflict"));
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
        append_change(target, change)?;
    }
    Ok(())
}

enum PlanChangeMatch {
    Absent,
    Identical,
    Conflict,
}

fn append_change(
    target: &mut ChangePlan,
    change: &ahcl_kit_core::Change,
) -> Result<(), RuntimeError> {
    match plan_change_match(target, change) {
        PlanChangeMatch::Absent => write_plan_change(target, change),
        PlanChangeMatch::Identical => Ok(()),
        PlanChangeMatch::Conflict => Err(RuntimeError::operation("materials.plan_conflict")),
    }
}

fn plan_change_match(target: &ChangePlan, change: &ahcl_kit_core::Change) -> PlanChangeMatch {
    let Some(existing) = target
        .changes()
        .into_iter()
        .find(|existing| existing.path() == change.path())
    else {
        return PlanChangeMatch::Absent;
    };
    if same_plan_change(existing, change) {
        PlanChangeMatch::Identical
    } else {
        PlanChangeMatch::Conflict
    }
}

fn same_plan_change(existing: &ahcl_kit_core::Change, change: &ahcl_kit_core::Change) -> bool {
    existing.kind() == change.kind() && existing.bytes() == change.bytes()
}

fn write_plan_change(
    target: &mut ChangePlan,
    change: &ahcl_kit_core::Change,
) -> Result<(), RuntimeError> {
    match change.kind() {
        ahcl_kit_core::ChangeKind::Create | ahcl_kit_core::ChangeKind::Replace => {
            write_plan_bytes(target, change)
        }
        ahcl_kit_core::ChangeKind::Remove => {
            target.remove(change.path().clone()).map_err(plan_error)
        }
    }
}

fn write_plan_bytes(
    target: &mut ChangePlan,
    change: &ahcl_kit_core::Change,
) -> Result<(), RuntimeError> {
    let bytes = change
        .bytes()
        .ok_or_else(|| RuntimeError::operation("materials.plan_missing_bytes"))?;
    target
        .write(change.path().clone(), bytes.to_vec())
        .map_err(plan_error)
}

pub fn system_utc_date() -> Result<UtcDate, RuntimeError> {
    let now = OffsetDateTime::now_utc();
    let year = u16::try_from(now.year())
        .map_err(|error| RuntimeError::with_source("cli.current_date", error))?;
    UtcDate::new(year, u8::from(now.month()), now.day())
        .map_err(|error| RuntimeError::with_source("cli.current_date", error))
}

fn load_config(
    project: &ProjectRoot,
    contributors: &[&'static dyn LanguageContributor],
) -> Result<EffectiveConfig, RuntimeError> {
    let filesystem = ProjectFilesystem::open(project.as_path()).map_err(materials_error)?;
    match config_entry(&filesystem)? {
        ProjectEntry::File(bytes) => parse_config(&bytes, contributors),
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

fn parse_config(
    bytes: &[u8],
    contributors: &[&'static dyn LanguageContributor],
) -> Result<EffectiveConfig, RuntimeError> {
    let document = ConfigDocument::parse_bytes_with(bytes, contributors)
        .map_err(|error| RuntimeError::with_source("config.invalid", error))?;
    EffectiveConfig::resolve(&document)
        .map_err(|error| RuntimeError::with_source("config.invalid", error))
}

fn prepared_config(
    identity: &ProjectIdentity,
    version: AhclVersion,
    current_date: UtcDate,
    contributors: &[&'static dyn LanguageContributor],
) -> Result<EffectiveConfig, RuntimeError> {
    let mut document =
        ConfigDocument::parse_with(&ConfigSkeleton::render_populated(identity), contributors)
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

fn prepare_opened_init(
    filesystem: &ProjectFilesystem,
    identity: Option<&ProjectIdentity>,
    current_date: UtcDate,
    contributors: &[&'static dyn LanguageContributor],
) -> Result<EffectiveConfig, RuntimeError> {
    match config_entry(filesystem)? {
        ProjectEntry::Absent => prepare_absent_init(identity, current_date, contributors),
        ProjectEntry::File(bytes) => {
            prepare_existing_init(&bytes, identity, current_date, contributors)
        }
        ProjectEntry::Other => Err(RuntimeError::operation("config.invalid")),
    }
}

fn prepare_absent_init(
    identity: Option<&ProjectIdentity>,
    current_date: UtcDate,
    contributors: &[&'static dyn LanguageContributor],
) -> Result<EffectiveConfig, RuntimeError> {
    prepared_config(
        required_identity(identity)?,
        AhclVersion::V1_2,
        current_date,
        contributors,
    )
}

fn prepare_existing_init(
    bytes: &[u8],
    identity: Option<&ProjectIdentity>,
    current_date: UtcDate,
    contributors: &[&'static dyn LanguageContributor],
) -> Result<EffectiveConfig, RuntimeError> {
    let existing = parse_config(bytes, contributors)?;
    if complete_identity(&existing) {
        return Ok(existing);
    }
    prepared_config(
        required_identity(identity)?,
        existing.license().version(),
        current_date,
        contributors,
    )
}

fn required_identity(identity: Option<&ProjectIdentity>) -> Result<&ProjectIdentity, RuntimeError> {
    identity.ok_or_else(|| RuntimeError::operation("config.identity_required"))
}

fn plan_config_init(
    filesystem: &ProjectFilesystem,
    force: bool,
    contributors: &[&'static dyn LanguageContributor],
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
        .write(
            path,
            ConfigSkeleton::render(contributors).as_bytes().to_vec(),
        )
        .map_err(|error| RuntimeError::with_source("materials.plan", error))?;
    let compared = desired
        .compare(filesystem)
        .map_err(|error| RuntimeError::with_source("materials.plan", error))?;
    Ok(RuntimePlan::from_changes(compared))
}

fn plan_project_init(
    filesystem: &ProjectFilesystem,
    request: &PlanRequest<'_>,
    contributors: &[&'static dyn LanguageContributor],
) -> Result<RuntimePlan, RuntimeError> {
    let license = required_license(request.license())?;
    let current_date = request
        .current_date()
        .ok_or_else(|| RuntimeError::operation("cli.current_date"))?;
    let (identity, force) = init_identity(filesystem, request, contributors)?;
    let changes = ProjectMaterialGenerator::plan_init(
        filesystem,
        identity,
        license,
        current_date,
        force,
        contributors,
    )
    .map_err(materials_error)?;
    Ok(RuntimePlan::from_changes(changes))
}

fn init_identity<'a>(
    filesystem: &ProjectFilesystem,
    request: &'a PlanRequest<'a>,
    contributors: &[&'static dyn LanguageContributor],
) -> Result<(Option<&'a ProjectIdentity>, bool), RuntimeError> {
    match config_entry(filesystem)? {
        ProjectEntry::Absent => Ok((request.identity(), false)),
        ProjectEntry::File(bytes) => existing_init_identity(&bytes, request, contributors),
        ProjectEntry::Other => Err(RuntimeError::operation("config.invalid")),
    }
}

fn existing_init_identity<'a>(
    bytes: &[u8],
    request: &'a PlanRequest<'a>,
    contributors: &[&'static dyn LanguageContributor],
) -> Result<(Option<&'a ProjectIdentity>, bool), RuntimeError> {
    let config = parse_config(bytes, contributors)?;
    if complete_identity(&config) {
        Ok((None, false))
    } else {
        Ok((request.identity(), true))
    }
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

fn adoption_date_or_current(
    config: &EffectiveConfig,
    current_date: Option<UtcDate>,
) -> Result<UtcDate, RuntimeError> {
    config
        .project()
        .adoption_date()
        .or(current_date)
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
