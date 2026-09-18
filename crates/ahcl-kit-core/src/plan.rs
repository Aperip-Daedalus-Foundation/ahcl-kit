use crate::{ProjectRoot, RepoPath};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::Path;

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
                    match read_existing_without_following(root, &destination, change.path()) {
                        Ok(existing) if existing == desired => {}
                        Ok(_) => compared.insert(change.clone().with_kind(ChangeKind::Replace))?,
                        Err(PlanError::MissingPath) => {
                            compared.insert(change.clone().with_kind(ChangeKind::Create))?
                        }
                        Err(error) => return Err(error),
                    }
                }
                ChangeKind::Remove => {
                    match inspect_existing_without_following(root, &destination, change.path()) {
                        Ok(_) => compared.insert(change.clone())?,
                        Err(PlanError::MissingPath) => {}
                        Err(error) => return Err(error),
                    }
                }
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

fn read_existing_without_following(
    root: &ProjectRoot,
    destination: &Path,
    path: &RepoPath,
) -> Result<Vec<u8>, PlanError> {
    let mut file = inspect_existing_without_following(root, destination, path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| PlanError::Read {
            path: path.clone(),
            source,
        })?;
    Ok(bytes)
}

fn inspect_existing_without_following(
    root: &ProjectRoot,
    destination: &Path,
    path: &RepoPath,
) -> Result<File, PlanError> {
    reject_linked_parent(root, path)?;
    match open_without_following(destination) {
        Ok(file) if is_reparse_point(&file, path)? => {
            Err(PlanError::UnsafePath { path: path.clone() })
        }
        Ok(file) => Ok(file),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Err(PlanError::MissingPath),
        Err(source) if is_link_error(&source) => Err(PlanError::UnsafePath { path: path.clone() }),
        Err(source) => Err(PlanError::Read {
            path: path.clone(),
            source,
        }),
    }
}

fn reject_linked_parent(root: &ProjectRoot, path: &RepoPath) -> Result<(), PlanError> {
    let mut current = root.as_path().to_path_buf();
    let mut components = path.as_str().split('/').peekable();
    while let Some(component) = components.next() {
        if components.peek().is_none() {
            break;
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_reparse_metadata(&metadata) => {
                return Err(PlanError::UnsafePath { path: path.clone() });
            }
            Ok(_) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(PlanError::Read {
                    path: path.clone(),
                    source,
                });
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn open_without_following(destination: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(destination)
}

#[cfg(windows)]
fn open_without_following(destination: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(destination)
}

#[cfg(not(any(unix, windows)))]
fn open_without_following(destination: &Path) -> std::io::Result<File> {
    OpenOptions::new().read(true).open(destination)
}

#[cfg(windows)]
fn is_reparse_point(file: &File, path: &RepoPath) -> Result<bool, PlanError> {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    let metadata = file.metadata().map_err(|source| PlanError::Read {
        path: path.clone(),
        source,
    })?;
    Ok(metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

#[cfg(not(windows))]
fn is_reparse_point(_: &File, _: &RepoPath) -> Result<bool, PlanError> {
    Ok(false)
}

#[cfg(unix)]
fn is_link_error(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(libc::ELOOP)
}

#[cfg(not(unix))]
fn is_link_error(_: &std::io::Error) -> bool {
    false
}

#[cfg(windows)]
fn is_reparse_metadata(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_metadata(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[derive(Debug)]
pub enum PlanError {
    ConflictingChange {
        path: RepoPath,
    },
    MissingBytes {
        path: RepoPath,
    },
    MissingPath,
    UnsafePath {
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
            Self::MissingPath => formatter.write_str("planned path does not exist"),
            Self::UnsafePath { path } => write!(formatter, "unsafe filesystem path for {path}"),
            Self::Read { path, source } => write!(formatter, "cannot compare {path}: {source}"),
        }
    }
}

impl std::error::Error for PlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::ConflictingChange { .. }
            | Self::MissingBytes { .. }
            | Self::MissingPath
            | Self::UnsafePath { .. } => None,
        }
    }
}
