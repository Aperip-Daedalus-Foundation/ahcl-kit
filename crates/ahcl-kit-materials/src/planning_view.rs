// crates/ahcl-kit-materials/src/planning_view.rs - First-read observations for project planning.
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

use crate::MaterialsError;
use ahcl_kit_core::{ChangePlan, ProjectEntry, ProjectView, ProjectViewError, RepoPath};
use std::cell::RefCell;
use std::collections::BTreeMap;

/// Merge and rendering share each path's first observation, not a later reread.
/// This cache is not a filesystem snapshot: finish validates against the live
/// capability, and application must validate again before making any writes.
pub(crate) struct PlanningView<'a> {
    source: &'a dyn ProjectView,
    entries: RefCell<BTreeMap<RepoPath, ProjectEntry>>,
}

impl<'a> PlanningView<'a> {
    pub(crate) fn new(source: &'a dyn ProjectView) -> Self {
        Self {
            source,
            entries: RefCell::new(BTreeMap::new()),
        }
    }

    pub(crate) fn finish(self, mut plan: ChangePlan) -> Result<ChangePlan, MaterialsError> {
        for (path, entry) in self.entries.into_inner() {
            plan.expect_entry(path, entry)
                .map_err(MaterialsError::from_plan)?;
        }
        // Never compare through the cache: that would hide intervening edits.
        plan.compare(self.source).map_err(MaterialsError::from_plan)
    }
}

impl ProjectView for PlanningView<'_> {
    fn entry(&self, path: &RepoPath) -> Result<ProjectEntry, ProjectViewError> {
        if let Some(entry) = self.entries.borrow().get(path) {
            return Ok(entry.clone());
        }
        let entry = self.source.entry(path)?;
        self.entries
            .borrow_mut()
            .insert(path.clone(), entry.clone());
        Ok(entry)
    }
}
