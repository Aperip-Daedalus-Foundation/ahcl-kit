use crate::{ProjectRoot, RepoPath};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;

/// The filesystem effect of one planned path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeKind {
    Create,
    Replace,
    Remove,
}

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

    /// Reads the selected project root and returns only changes that would alter it.
    pub fn compare(&self, root: &ProjectRoot) -> Result<Self, PlanError> {
        let mut compared = Self::new();
        for change in self.changes.values() {
            let destination = root.resolve(change.path());
            match change.kind() {
                ChangeKind::Create | ChangeKind::Replace => {
                    let desired = change.bytes().ok_or_else(|| PlanError::MissingBytes {
                        path: change.path().clone(),
                    })?;
                    match fs::read(&destination) {
                        Ok(existing) if existing == desired => {}
                        Ok(_) => compared.insert(change.clone().with_kind(ChangeKind::Replace))?,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            compared.insert(change.clone().with_kind(ChangeKind::Create))?
                        }
                        Err(source) => {
                            return Err(PlanError::Read {
                                path: change.path().clone(),
                                source,
                            });
                        }
                    }
                }
                ChangeKind::Remove => match fs::symlink_metadata(&destination) {
                    Ok(_) => compared.insert(change.clone())?,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(source) => {
                        return Err(PlanError::Read {
                            path: change.path().clone(),
                            source,
                        });
                    }
                },
            }
        }
        Ok(compared)
    }

    fn insert(&mut self, change: Change) -> Result<(), PlanError> {
        let path = change.path().clone();
        if self.changes.contains_key(&path) {
            return Err(PlanError::ConflictingChange { path });
        }
        self.changes.insert(path, change);
        Ok(())
    }
}

#[derive(Debug)]
pub enum PlanError {
    ConflictingChange {
        path: RepoPath,
    },
    MissingBytes {
        path: RepoPath,
    },
    Read {
        path: RepoPath,
        source: std::io::Error,
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
            Self::Read { path, source } => write!(formatter, "cannot compare {path}: {source}"),
        }
    }
}

impl std::error::Error for PlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::ConflictingChange { .. } | Self::MissingBytes { .. } => None,
        }
    }
}
