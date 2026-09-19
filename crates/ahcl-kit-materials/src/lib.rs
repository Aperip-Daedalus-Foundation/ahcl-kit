//! Filesystem-neutral AHCL project layout and rendering plans.

mod dependencies;
mod layout;
mod project;
mod render;
mod third_party;

pub use dependencies::DependencyMaterialGenerator;
pub use layout::LayoutPolicy;
pub use project::{MaterialsError, MaterialsErrorCode, ProjectMaterialGenerator};
pub use third_party::{
    ManagedEntryKind, ManagedEvidenceInventory, ManagedPackageInventory, ManagedRemoval,
    ManagedRootInventoryEntry, ManagedThirdPartyInventory, MaterialGenerationPlan,
    ThirdPartyMaterialGenerator,
};

use ahcl_kit_config::EffectiveConfig;
use ahcl_kit_core::{ChangePlan, ProjectView, RepoPath, ResolvedGraph, UtcDate};
use ahcl_kit_license::VerifiedLicense;
use std::collections::BTreeMap;

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
        let layout = LayoutPolicy::from_config(config)?;
        let dependency_path = layout.dependencies_path()?;
        let project =
            ProjectMaterialGenerator::plan_project_files(view, config, license, current_date)?;
        let license_plan = ProjectMaterialGenerator::plan_license_sync(view, config, license)?;
        let dependency = DependencyMaterialGenerator::plan_document(view, config, graph)?;
        let third_party = ThirdPartyMaterialGenerator::plan_tree(view, config, graph, inventory)?;
        let (third_party_changes, removals, diagnostics) = third_party.into_parts();
        let mut writes = BTreeMap::new();
        collect_writes(&mut writes, &project, Some(&dependency_path))?;
        collect_writes(&mut writes, &license_plan, None)?;
        collect_writes(&mut writes, &dependency, None)?;
        collect_writes(&mut writes, &third_party_changes, None)?;
        let mut desired = ChangePlan::new();
        for (path, bytes) in writes {
            desired
                .write(path, bytes)
                .map_err(MaterialsError::from_plan)?;
        }
        let changes = desired.compare(view).map_err(MaterialsError::from_plan)?;
        Ok(MaterialGenerationPlan::new(changes, removals, diagnostics))
    }
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
