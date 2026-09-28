// crates/ahcl-kit-materials/src/lib.rs - Public API for material planning and application.
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

//! Deterministic AHCL planning and capability-scoped material application.

mod apply;
mod dependencies;
mod layout;
mod platform_fs;
mod project;
mod render;
mod third_party;
mod view;

pub use apply::PlanApplier;
pub use dependencies::DependencyMaterialGenerator;
pub use layout::LayoutPolicy;
pub use project::{MaterialsError, MaterialsErrorCode, ProjectMaterialGenerator};
pub use third_party::{
    ManagedEntryKind, ManagedEvidenceInventory, ManagedPackageInventory, ManagedRemoval,
    ManagedRootInventoryEntry, ManagedThirdPartyInventory, MaterialGenerationPlan,
    ThirdPartyMaterialGenerator,
};
pub use view::{ManagedThirdPartyDir, ProjectFilesystem};

use ahcl_kit_config::EffectiveConfig;
use ahcl_kit_core::{ChangePlan, ProjectView, RepoPath, ResolvedGraph, UtcDate};
use ahcl_kit_fs::is_safe_component;
use ahcl_kit_license::VerifiedLicense;
use std::collections::BTreeMap;
use std::ffi::OsString;

pub struct ProjectGenerationPlanner;

impl ProjectGenerationPlanner {
    pub fn plan_all(
        view: &dyn ProjectView,
        config: &EffectiveConfig,
        license: &VerifiedLicense,
        graph: &ResolvedGraph,
        inventory: &ManagedThirdPartyInventory,
        current_date: UtcDate,
    ) -> Result<MaterialGenerationPlan, MaterialsError> {
        let planned = plan_documents(PlanInputs {
            view,
            config,
            license,
            graph,
            inventory,
            current_date,
        })?;
        assemble_plan(view, planned)
    }
}

struct PlanInputs<'a> {
    view: &'a dyn ProjectView,
    config: &'a EffectiveConfig,
    license: &'a VerifiedLicense,
    graph: &'a ResolvedGraph,
    inventory: &'a ManagedThirdPartyInventory,
    current_date: UtcDate,
}

struct PlannedDocuments {
    writes: BTreeMap<RepoPath, Vec<u8>>,
    removals: Vec<crate::ManagedRemoval>,
    diagnostics: Vec<ahcl_kit_core::Diagnostic>,
}

fn plan_documents(input: PlanInputs<'_>) -> Result<PlannedDocuments, MaterialsError> {
    let layout = LayoutPolicy::from_config(input.config)?;
    let dependency_path = layout.dependencies_path()?;
    let documents = generate_documents(&input)?;
    let writes = collect_document_writes(&documents, &dependency_path)?;
    Ok(PlannedDocuments {
        writes,
        removals: documents.removals,
        diagnostics: documents.diagnostics,
    })
}

struct GeneratedDocuments {
    project: ChangePlan,
    license_plan: ChangePlan,
    dependency: ChangePlan,
    third_party: ChangePlan,
    removals: Vec<crate::ManagedRemoval>,
    diagnostics: Vec<ahcl_kit_core::Diagnostic>,
}

fn generate_documents(input: &PlanInputs<'_>) -> Result<GeneratedDocuments, MaterialsError> {
    let project = ProjectMaterialGenerator::plan_project_files(
        input.view,
        input.config,
        input.license,
        input.current_date,
    )?;
    let license_plan =
        ProjectMaterialGenerator::plan_license_sync(input.view, input.config, input.license)?;
    let dependency =
        DependencyMaterialGenerator::plan_document(input.view, input.config, input.graph)?;
    let third_party = ThirdPartyMaterialGenerator::plan_tree(
        input.view,
        input.config,
        input.graph,
        input.inventory,
    )?;
    let (third_party_changes, removals, diagnostics) = third_party.into_parts();
    Ok(GeneratedDocuments {
        project,
        license_plan,
        dependency,
        third_party: third_party_changes,
        removals,
        diagnostics,
    })
}

fn collect_document_writes(
    documents: &GeneratedDocuments,
    dependency_path: &RepoPath,
) -> Result<BTreeMap<RepoPath, Vec<u8>>, MaterialsError> {
    let mut writes = BTreeMap::new();
    collect_writes(&mut writes, &documents.project, Some(dependency_path))?;
    collect_writes(&mut writes, &documents.license_plan, None)?;
    collect_writes(&mut writes, &documents.dependency, None)?;
    collect_writes(&mut writes, &documents.third_party, None)?;
    Ok(writes)
}

fn assemble_plan(
    view: &dyn ProjectView,
    planned: PlannedDocuments,
) -> Result<MaterialGenerationPlan, MaterialsError> {
    let desired = desired_plan(planned.writes)?;
    let changes = desired.compare(view).map_err(MaterialsError::from_plan)?;
    Ok(MaterialGenerationPlan::new(
        changes,
        planned.removals,
        planned.diagnostics,
    ))
}

fn desired_plan(writes: BTreeMap<RepoPath, Vec<u8>>) -> Result<ChangePlan, MaterialsError> {
    let mut desired = ChangePlan::new();
    for (path, bytes) in writes {
        desired
            .write(path, bytes)
            .map_err(MaterialsError::from_plan)?;
    }
    Ok(desired)
}

fn collect_writes(
    target: &mut BTreeMap<RepoPath, Vec<u8>>,
    plan: &ChangePlan,
    skip: Option<&RepoPath>,
) -> Result<(), MaterialsError> {
    for change in plan.changes() {
        if skip.is_some_and(|path| path == change.path()) {
            continue;
        }
        let bytes = change
            .bytes()
            .ok_or_else(|| MaterialsError::new(MaterialsErrorCode::Plan))?;
        if let Some(existing) = target.get(change.path()) {
            if existing != bytes {
                return Err(MaterialsError::new(MaterialsErrorCode::Plan));
            }
            continue;
        }
        target.insert(change.path().clone(), bytes.to_vec());
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub(crate) struct SafeRelPath {
    repo_path: RepoPath,
    components: Vec<OsString>,
}

impl SafeRelPath {
    pub(crate) fn from_repo_path(path: &RepoPath) -> Result<Self, MaterialsError> {
        let value = path.as_str();
        if !relative_shape_ok(value) {
            return Err(Self::invalid(path));
        }

        let mut components = Vec::new();
        for component in value.split(['/', '\\']) {
            push_safe_component(&mut components, component, path)?;
        }
        if components.is_empty() {
            return Err(Self::invalid(path));
        }

        Ok(Self {
            repo_path: path.clone(),
            components,
        })
    }

    pub(crate) fn managed_namespace(materials: &RepoPath) -> Result<Self, MaterialsError> {
        let mut path = materials.as_str().to_owned();
        path.push_str("/THIRD-PARTY-LICENSES");
        let repo_path = RepoPath::parse(path).map_err(|_| {
            MaterialsError::filesystem_at(
                "materials.path.invalid",
                "project-relative path is invalid",
                materials.clone(),
            )
        })?;
        Self::from_repo_path(&repo_path)
    }

    pub(crate) fn components(&self) -> &[OsString] {
        &self.components
    }

    pub(crate) fn repo_path(&self) -> &RepoPath {
        &self.repo_path
    }

    fn invalid(path: &RepoPath) -> MaterialsError {
        MaterialsError::filesystem_at(
            "materials.path.invalid",
            "project-relative path is invalid",
            path.clone(),
        )
    }
}

fn relative_shape_ok(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.starts_with('\\')
        && !has_windows_prefix(value)
}

fn push_safe_component(
    components: &mut Vec<OsString>,
    component: &str,
    path: &RepoPath,
) -> Result<(), MaterialsError> {
    if !is_safe_component(component) {
        return Err(SafeRelPath::invalid(path));
    }
    components.push(OsString::from(component));
    Ok(())
}

fn has_windows_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}
