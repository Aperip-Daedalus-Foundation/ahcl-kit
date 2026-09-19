// crates/ahcl-kit-cli/src/runtime.rs - Object-safe command runtime contracts and plans.
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

use ahcl_kit_config::{AhclVersion, EffectiveConfig, ProjectIdentity};
use ahcl_kit_core::{ChangePlan, Diagnostic, ProjectRoot, RepoPath, ResolvedGraph, UtcDate};
use ahcl_kit_license::VerifiedLicense;
use ahcl_kit_materials::ManagedRemoval;
use std::error::Error;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterKind {
    Cargo,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanScope {
    ConfigInit,
    ProjectInit,
    ProjectFiles,
    License,
    Dependencies,
    ThirdParty,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedAdapter {
    adapter: AdapterKind,
    graph: ResolvedGraph,
}

impl ResolvedAdapter {
    pub fn new(adapter: AdapterKind, graph: ResolvedGraph) -> Self {
        Self { adapter, graph }
    }

    pub fn adapter(&self) -> AdapterKind {
        self.adapter
    }

    pub fn graph(&self) -> &ResolvedGraph {
        &self.graph
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlanRequest<'a> {
    scopes: &'a [PlanScope],
    config: Option<&'a EffectiveConfig>,
    license: Option<&'a VerifiedLicense>,
    adapters: &'a [ResolvedAdapter],
    current_date: Option<UtcDate>,
    force: bool,
    identity: Option<&'a ProjectIdentity>,
}

impl<'a> PlanRequest<'a> {
    pub(crate) fn new(
        scopes: &'a [PlanScope],
        config: Option<&'a EffectiveConfig>,
        license: Option<&'a VerifiedLicense>,
        adapters: &'a [ResolvedAdapter],
        current_date: Option<UtcDate>,
        force: bool,
        identity: Option<&'a ProjectIdentity>,
    ) -> Self {
        Self {
            scopes,
            config,
            license,
            adapters,
            current_date,
            force,
            identity,
        }
    }

    pub fn scopes(&self) -> &[PlanScope] {
        self.scopes
    }

    pub fn config(&self) -> Option<&EffectiveConfig> {
        self.config
    }

    pub fn license(&self) -> Option<&VerifiedLicense> {
        self.license
    }

    pub fn adapters(&self) -> &[ResolvedAdapter] {
        self.adapters
    }

    pub fn current_date(&self) -> Option<UtcDate> {
        self.current_date
    }

    pub fn force(&self) -> bool {
        self.force
    }

    pub fn identity(&self) -> Option<&ProjectIdentity> {
        self.identity
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RuntimePlan {
    changes: ChangePlan,
    managed_removals: Vec<ManagedRemoval>,
    diagnostics: Vec<Diagnostic>,
    managed_materials_directory: Option<RepoPath>,
}

impl RuntimePlan {
    pub fn from_changes(changes: ChangePlan) -> Self {
        Self {
            changes,
            ..Self::default()
        }
    }

    pub(crate) fn new(
        changes: ChangePlan,
        managed_removals: Vec<ManagedRemoval>,
        diagnostics: Vec<Diagnostic>,
        managed_materials_directory: Option<RepoPath>,
    ) -> Self {
        Self {
            changes,
            managed_removals,
            diagnostics,
            managed_materials_directory,
        }
    }

    pub fn changes(&self) -> &ChangePlan {
        &self.changes
    }

    pub fn managed_removals(&self) -> &[ManagedRemoval] {
        &self.managed_removals
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn managed_materials_directory(&self) -> Option<&RepoPath> {
        self.managed_materials_directory.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.managed_removals.is_empty()
    }

    pub(crate) fn with_changes(self, changes: ChangePlan) -> Self {
        Self { changes, ..self }
    }
}

pub trait CommandRuntime {
    fn load_config(&mut self, project: &ProjectRoot) -> Result<EffectiveConfig, RuntimeError>;

    fn prepare_project_init(
        &mut self,
        project: &ProjectRoot,
        identity: Option<&ProjectIdentity>,
        current_date: UtcDate,
    ) -> Result<EffectiveConfig, RuntimeError>;

    fn current_utc_date(&mut self) -> Result<UtcDate, RuntimeError>;

    fn fetch_official_license(
        &mut self,
        version: AhclVersion,
    ) -> Result<VerifiedLicense, RuntimeError>;

    fn resolve_adapter(
        &mut self,
        adapter: AdapterKind,
        project: &ProjectRoot,
        config: &EffectiveConfig,
    ) -> Result<ResolvedGraph, RuntimeError>;

    fn plan(
        &mut self,
        project: &ProjectRoot,
        request: PlanRequest<'_>,
    ) -> Result<ChangePlan, RuntimeError>;

    fn compare(
        &mut self,
        project: &ProjectRoot,
        plan: &ChangePlan,
    ) -> Result<ChangePlan, RuntimeError>;

    fn apply(&mut self, project: &ProjectRoot, plan: &ChangePlan) -> Result<(), RuntimeError>;

    fn plan_project(
        &mut self,
        project: &ProjectRoot,
        request: PlanRequest<'_>,
    ) -> Result<RuntimePlan, RuntimeError> {
        self.plan(project, request).map(RuntimePlan::from_changes)
    }

    fn compare_project(
        &mut self,
        project: &ProjectRoot,
        plan: RuntimePlan,
    ) -> Result<RuntimePlan, RuntimeError> {
        let changes = self.compare(project, plan.changes())?;
        Ok(plan.with_changes(changes))
    }

    fn apply_project(
        &mut self,
        project: &ProjectRoot,
        plan: &RuntimePlan,
    ) -> Result<(), RuntimeError> {
        if !plan.managed_removals().is_empty() {
            return Err(RuntimeError::operation("cli.managed_removals_unsupported"));
        }
        self.apply(project, plan.changes())
    }
}

pub struct RuntimeError {
    code: String,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl fmt::Debug for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeError")
            .field("code", &self.code)
            .field("message", &"runtime operation failed")
            .finish()
    }
}

impl RuntimeError {
    pub fn operation(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            source: None,
        }
    }

    pub fn with_source(
        code: impl Into<String>,
        source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            code: code.into(),
            source: Some(Box::new(source)),
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("runtime operation failed")
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UnavailableRuntime;

impl CommandRuntime for UnavailableRuntime {
    fn load_config(&mut self, _: &ProjectRoot) -> Result<EffectiveConfig, RuntimeError> {
        Err(unavailable())
    }

    fn prepare_project_init(
        &mut self,
        _: &ProjectRoot,
        _: Option<&ProjectIdentity>,
        _: UtcDate,
    ) -> Result<EffectiveConfig, RuntimeError> {
        Err(unavailable())
    }

    fn current_utc_date(&mut self) -> Result<UtcDate, RuntimeError> {
        Err(unavailable())
    }

    fn fetch_official_license(&mut self, _: AhclVersion) -> Result<VerifiedLicense, RuntimeError> {
        Err(unavailable())
    }

    fn resolve_adapter(
        &mut self,
        _: AdapterKind,
        _: &ProjectRoot,
        _: &EffectiveConfig,
    ) -> Result<ResolvedGraph, RuntimeError> {
        Err(unavailable())
    }

    fn plan(&mut self, _: &ProjectRoot, _: PlanRequest<'_>) -> Result<ChangePlan, RuntimeError> {
        Err(unavailable())
    }

    fn compare(&mut self, _: &ProjectRoot, _: &ChangePlan) -> Result<ChangePlan, RuntimeError> {
        Err(unavailable())
    }

    fn apply(&mut self, _: &ProjectRoot, _: &ChangePlan) -> Result<(), RuntimeError> {
        Err(unavailable())
    }
}

fn unavailable() -> RuntimeError {
    RuntimeError::operation("cli.runtime_unavailable")
}
