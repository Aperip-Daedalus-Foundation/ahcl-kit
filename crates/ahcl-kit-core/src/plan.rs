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
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChangePlan {
    changes: BTreeMap<RepoPath, Change>,
    portable_paths: BTreeMap<String, RepoPath>,
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

    /// Compares the plan against capability-owned project observations.
    pub fn compare(&self, view: &dyn ProjectView) -> Result<Self, PlanError> {
        let mut compared = Self::new();
        for change in self.changes.values() {
            let entry = view
                .entry(change.path())
                .map_err(|source| PlanError::View {
                    path: change.path().clone(),
                    source,
                })?;
            match change.kind() {
                ChangeKind::Create | ChangeKind::Replace => {
                    let desired = change.bytes().ok_or_else(|| PlanError::MissingBytes {
                        path: change.path().clone(),
                    })?;
                    match entry {
                        ProjectEntry::Absent => {
                            compared.insert(change.clone().with_kind(ChangeKind::Create))?
                        }
                        ProjectEntry::File(existing) if existing == desired => {}
                        ProjectEntry::File(_) => {
                            compared.insert(change.clone().with_kind(ChangeKind::Replace))?
                        }
                        ProjectEntry::Other => {
                            return Err(PlanError::UnsupportedEntry {
                                path: change.path().clone(),
                            });
                        }
                    }
                }
                ChangeKind::Remove => match entry {
                    ProjectEntry::Absent => {}
                    ProjectEntry::File(_) | ProjectEntry::Other => {
                        compared.insert(change.clone())?
                    }
                },
            }
        }
        Ok(compared)
    }

    fn insert(&mut self, change: Change) -> Result<(), PlanError> {
        let path = change.path().clone();
        let portable_key = portable_path_key(&path);
        if self.changes.contains_key(&path) || self.portable_paths.contains_key(&portable_key) {
            return Err(PlanError::ConflictingChange { path });
        }
        self.portable_paths.insert(portable_key, path.clone());
        self.changes.insert(path, change);
        Ok(())
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
            | Self::MissingBytes { .. }
            | Self::UnsupportedEntry { .. } => None,
        }
    }
}
