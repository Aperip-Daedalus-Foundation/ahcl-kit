use crate::view::ProjectFilesystem;
use crate::{MaterialsError, SafeRelPath};
use ahcl_kit_core::{ChangeKind, ChangePlan};

pub struct PlanApplier;

impl PlanApplier {
    pub fn apply(
        filesystem: &ProjectFilesystem,
        compared_plan: &ChangePlan,
    ) -> Result<(), MaterialsError> {
        let mut writes = Vec::new();
        for change in compared_plan.changes() {
            if change.kind() == ChangeKind::Remove {
                return Err(MaterialsError::root(
                    "materials.apply.remove_forbidden",
                    "general planned removals are forbidden",
                ));
            }
            let path = SafeRelPath::from_repo_path(change.path())?;
            let bytes = change.bytes().ok_or_else(|| {
                MaterialsError::at_path(
                    "materials.apply.missing_bytes",
                    "planned write has no content",
                    &path,
                )
            })?;
            writes.push((path, change.kind(), bytes));
        }

        for (path, kind, bytes) in writes {
            let replace = kind == ChangeKind::Replace;
            filesystem.root.atomic_write(&path, bytes, replace)?;
        }
        Ok(())
    }
}
