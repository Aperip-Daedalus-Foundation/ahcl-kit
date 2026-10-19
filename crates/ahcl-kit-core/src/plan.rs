// crates/ahcl-kit-core/src/plan.rs - Deterministic change planning and comparison.
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

use crate::RepoPath;
use std::collections::BTreeMap;
use std::fmt;

/// The filesystem effect of one planned path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeKind {
    Create,
    Replace,
    Remove,
}

/// A filesystem-neutral observation of one repository-relative path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectEntry {
    Absent,
    File(Vec<u8>),
    Other,
}

/// A capability-owned source of project entry observations.
pub trait ProjectView {
    fn entry(&self, path: &RepoPath) -> Result<ProjectEntry, ProjectViewError>;
}

/// A sanitized failure returned by a project view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectViewError {
    message: String,
}

impl ProjectViewError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ProjectViewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ProjectViewError {}

/// One repository-relative filesystem intention.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Change {
    path: RepoPath,
    kind: ChangeKind,
    bytes: Option<Vec<u8>>,
}

impl Change {
    fn write(path: RepoPath, bytes: Vec<u8>) -> Self {
        Self {
            path,
            kind: ChangeKind::Create,
            bytes: Some(bytes),
        }
    }

    fn remove(path: RepoPath) -> Self {
        Self {
            path,
            kind: ChangeKind::Remove,
            bytes: None,
        }
    }

    fn with_kind(mut self, kind: ChangeKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn path(&self) -> &RepoPath {
        &self.path
    }

    pub fn kind(&self) -> ChangeKind {
        self.kind
    }

    pub fn bytes(&self) -> Option<&[u8]> {
        self.bytes.as_deref()
    }
}

/// A byte-stably ordered set of changes which can be compared without writing.
///
/// Expected entries retain the first planning observations, including read-only
/// dependencies and writes suppressed as unchanged. Validation is optimistic:
/// it neither locks entries nor provides an atomic compare-and-swap or transaction.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChangePlan {
    changes: BTreeMap<RepoPath, Change>,
    portable_paths: BTreeMap<String, RepoPath>,
    expected_entries: BTreeMap<RepoPath, ProjectEntry>,
}

impl ChangePlan {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write(&mut self, path: RepoPath, bytes: Vec<u8>) -> Result<(), PlanError> {
        self.insert(Change::write(path, bytes))
    }

    pub fn remove(&mut self, path: RepoPath) -> Result<(), PlanError> {
        self.insert(Change::remove(path))
    }

    pub fn changes(&self) -> Vec<&Change> {
        self.changes.values().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Records a planning input without replacing an earlier observation.
    /// Case-only read probes may coexist, but must not alias a planned change.
    pub fn expect_entry(&mut self, path: RepoPath, entry: ProjectEntry) -> Result<(), PlanError> {
        if let Some(expected) = self.expected_entries.get(&path) {
            if expected != &entry {
                return Err(PlanError::ConflictingObservation { path });
            }
        }
        self.check_reserved_path(&path)?;
        self.expected_entries.entry(path).or_insert(entry);
        Ok(())
    }

    /// Includes observations for inputs whose own output is unchanged.
    pub fn expected_entries(&self) -> impl Iterator<Item = (&RepoPath, &ProjectEntry)> {
        self.expected_entries.iter()
    }

    /// Carries read dependencies through plan composition; disagreements fail.
    pub fn extend_expectations(&mut self, source: &Self) -> Result<(), PlanError> {
        for (path, entry) in source.expected_entries() {
            self.expect_entry(path.clone(), entry.clone())?;
        }
        Ok(())
    }

    /// Validates the entire read set without changing its baseline or writing.
    pub fn validate_expectations(&self, view: &dyn ProjectView) -> Result<(), PlanError> {
        for (path, expected) in self.expected_entries() {
            validate_expected(view, path, expected)?;
        }
        Ok(())
    }

    /// Rechecks a guarded target immediately before a caller commits it.
    /// An unobserved target has no guard; callers must still enforce create-only
    /// and no-follow rules. A later concurrent write can still race this check.
    pub fn validate_expected_entry(
        &self,
        view: &dyn ProjectView,
        path: &RepoPath,
    ) -> Result<(), PlanError> {
        if let Some(expected) = self.expected_entries.get(path) {
            validate_expected(view, path, expected)?;
        }
        Ok(())
    }

    /// Compares against live observations, preserving and validating prior inputs.
    pub fn compare(&self, view: &dyn ProjectView) -> Result<Self, PlanError> {
        self.validate_expectations(view)?;
        let mut compared = Self::new();
        compared.extend_expectations(self)?;
        for change in self.changes.values() {
            compared.classify_change(change, view)?;
        }
        Ok(compared)
    }

    fn classify_change(
        &mut self,
        change: &Change,
        view: &dyn ProjectView,
    ) -> Result<(), PlanError> {
        let entry = match self.expected_entries.get(change.path()) {
            Some(expected) => expected.clone(),
            None => observed_entry(view, change.path())?,
        };
        self.expect_entry(change.path().clone(), entry.clone())?;
        // Writes and removals stay on separate paths: equal bytes suppress a
        // write, while any present entry still requires an explicit remove.
        match change.kind() {
            ChangeKind::Create | ChangeKind::Replace => self.compare_write(change, entry),
            ChangeKind::Remove => self.compare_remove(change, entry),
        }
    }

    fn compare_write(&mut self, change: &Change, entry: ProjectEntry) -> Result<(), PlanError> {
        let desired = change.bytes().ok_or_else(|| PlanError::MissingBytes {
            path: change.path().clone(),
        })?;
        let Some(kind) = write_kind(entry, desired, change.path())? else {
            return Ok(());
        };
        self.insert(change.clone().with_kind(kind))
    }

    fn compare_remove(&mut self, change: &Change, entry: ProjectEntry) -> Result<(), PlanError> {
        if remove_still_required(entry) {
            self.insert(change.clone())
        } else {
            Ok(())
        }
    }

    fn insert(&mut self, change: Change) -> Result<(), PlanError> {
        let path = change.path().clone();
        if self.changes.contains_key(&path) {
            return Err(PlanError::ConflictingChange { path });
        }
        let portable_key = portable_path_key(&path);
        if self
            .expected_entries
            .keys()
            .any(|expected| expected != &path && portable_path_key(expected) == portable_key)
        {
            return Err(PlanError::ConflictingChange { path });
        }
        self.reserve_path(&path)?;
        self.changes.insert(path, change);
        Ok(())
    }

    fn reserve_path(&mut self, path: &RepoPath) -> Result<(), PlanError> {
        self.check_reserved_path(path)?;
        self.portable_paths
            .insert(portable_path_key(path), path.clone());
        Ok(())
    }

    fn check_reserved_path(&self, path: &RepoPath) -> Result<(), PlanError> {
        if self
            .portable_paths
            .get(&portable_path_key(path))
            .is_some_and(|existing| existing != path)
        {
            return Err(PlanError::ConflictingChange { path: path.clone() });
        }
        Ok(())
    }
}

fn observed_entry(view: &dyn ProjectView, path: &RepoPath) -> Result<ProjectEntry, PlanError> {
    view.entry(path).map_err(|source| PlanError::View {
        path: path.clone(),
        source,
    })
}

fn validate_expected(
    view: &dyn ProjectView,
    path: &RepoPath,
    expected: &ProjectEntry,
) -> Result<(), PlanError> {
    let entry = view
        .entry(path)
        .map_err(|_| PlanError::StaleEntry { path: path.clone() })?;
    if &entry != expected {
        return Err(PlanError::StaleEntry { path: path.clone() });
    }
    Ok(())
}

fn write_kind(
    entry: ProjectEntry,
    desired: &[u8],
    path: &RepoPath,
) -> Result<Option<ChangeKind>, PlanError> {
    match entry {
        ProjectEntry::Absent => Ok(Some(ChangeKind::Create)),
        ProjectEntry::File(existing) if existing == desired => Ok(None),
        ProjectEntry::File(_) => Ok(Some(ChangeKind::Replace)),
        ProjectEntry::Other => Err(PlanError::UnsupportedEntry { path: path.clone() }),
    }
}

fn remove_still_required(entry: ProjectEntry) -> bool {
    match entry {
        ProjectEntry::Absent => false,
        ProjectEntry::File(_) | ProjectEntry::Other => true,
    }
}

fn portable_path_key(path: &RepoPath) -> String {
    path.as_str().to_lowercase()
}

#[derive(Debug)]
pub enum PlanError {
    ConflictingChange {
        path: RepoPath,
    },
    ConflictingObservation {
        path: RepoPath,
    },
    StaleEntry {
        path: RepoPath,
    },
    MissingBytes {
        path: RepoPath,
    },
    UnsupportedEntry {
        path: RepoPath,
    },
    View {
        path: RepoPath,
        source: ProjectViewError,
    },
}

impl fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConflictingChange { path } => {
                write!(formatter, "duplicate planned change for {path}")
            }
            Self::ConflictingObservation { path } => {
                write!(
                    formatter,
                    "planning inputs disagree for {path}; replan before applying"
                )
            }
            Self::StaleEntry { path } => {
                write!(
                    formatter,
                    "planning input changed or cannot be verified for {path}; replan before applying"
                )
            }
            Self::MissingBytes { path } => {
                write!(formatter, "write change is missing bytes for {path}")
            }
            Self::UnsupportedEntry { path } => {
                write!(formatter, "unsupported project entry for {path}")
            }
            Self::View { path, source } => write!(formatter, "cannot compare {path}: {source}"),
        }
    }
}

impl std::error::Error for PlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::View { source, .. } => Some(source),
            Self::ConflictingChange { .. }
            | Self::ConflictingObservation { .. }
            | Self::StaleEntry { .. }
            | Self::MissingBytes { .. }
            | Self::UnsupportedEntry { .. } => None,
        }
    }
}
