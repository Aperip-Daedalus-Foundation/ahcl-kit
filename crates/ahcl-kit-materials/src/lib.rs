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
use ahcl_kit_license::VerifiedLicense;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};

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

#[derive(Clone, Debug)]
pub(crate) struct SafeRelPath {
    repo_path: RepoPath,
    components: Vec<OsString>,
}

impl SafeRelPath {
    pub(crate) fn from_repo_path(path: &RepoPath) -> Result<Self, MaterialsError> {
        let value = path.as_str();
        if value.is_empty()
            || value.starts_with('/')
            || value.starts_with('\\')
            || has_windows_prefix(value)
        {
            return Err(Self::invalid(path));
        }

        let mut components = Vec::new();
        for component in value.split(['/', '\\']) {
            if !is_safe_component(component) {
                return Err(Self::invalid(path));
            }
            components.push(OsString::from(component));
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

fn has_windows_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn is_safe_component(component: &str) -> bool {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.as_bytes().contains(&0)
        || component.ends_with([' ', '.'])
        || component.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\'
                )
        })
    {
        return false;
    }

    let base_name = match component.split_once('.') {
        Some((name, _)) => name,
        None => component,
    }
    .to_ascii_uppercase();
    if matches!(base_name.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return false;
    }

    let suffix = match base_name
        .strip_prefix("COM")
        .or_else(|| base_name.strip_prefix("LPT"))
    {
        Some(value) => value,
        None => return true,
    };
    !matches!(
        suffix,
        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "\u{b9}" | "\u{b2}" | "\u{b3}"
    )
}

pub(crate) fn is_safe_os_component(component: &OsStr) -> bool {
    match component.to_str() {
        Some(value) => is_safe_component(value),
        None => false,
    }
}

pub(crate) fn temp_component() -> Result<OsString, MaterialsError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|_| {
        MaterialsError::filesystem(
            "materials.random.unavailable",
            "secure temporary-name generation failed",
        )
    })?;
    let mut name = String::with_capacity(14 + random.len() * 2);
    name.push_str(".ahcl-kit-tmp-");
    for byte in random {
        name.push(char::from(HEX[usize::from(byte >> 4)]));
        name.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(OsString::from(name))
}

#[cfg(unix)]
pub(crate) fn is_dot_entry(name: &OsStr) -> bool {
    name == OsStr::new(".") || name == OsStr::new("..")
}
