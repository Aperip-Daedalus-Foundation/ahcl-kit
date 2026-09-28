// crates/ahcl-kit-cli/src/discovery.rs - Project discovery and path normalization.
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

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryMode {
    ExistingConfig,
    Initialization,
}

pub(crate) fn resolve_projects(
    initial_cwd: &Path,
    positional: &[PathBuf],
    projects_from: Option<&Path>,
    mode: DiscoveryMode,
) -> Result<Vec<PathBuf>, DiscoveryError> {
    let entries = project_entries(initial_cwd, positional, projects_from)?;
    if entries.is_empty() {
        return discover_default(initial_cwd, mode).map(|path| vec![path]);
    }

    let mut projects = BTreeMap::new();
    for entry in entries {
        insert_project(initial_cwd, entry, &mut projects)?;
    }
    Ok(projects.into_values().collect())
}

fn project_entries(
    initial_cwd: &Path,
    positional: &[PathBuf],
    projects_from: Option<&Path>,
) -> Result<Vec<ProjectEntry>, DiscoveryError> {
    let mut entries = positional
        .iter()
        .cloned()
        .map(|path| ProjectEntry { path, line: None })
        .collect::<Vec<_>>();
    if let Some(file) = projects_from {
        entries.extend(read_projects_file(initial_cwd, file)?);
    }
    Ok(entries)
}

fn insert_project(
    initial_cwd: &Path,
    entry: ProjectEntry,
    projects: &mut BTreeMap<String, PathBuf>,
) -> Result<(), DiscoveryError> {
    let requested = if entry.path.is_absolute() {
        entry.path
    } else {
        initial_cwd.join(entry.path)
    };
    let canonical = fs::canonicalize(&requested).map_err(|source| DiscoveryError::ProjectPath {
        path: requested.clone(),
        line: entry.line,
        source,
    })?;
    let metadata = fs::metadata(&canonical).map_err(|source| DiscoveryError::ProjectPath {
        path: requested.clone(),
        line: entry.line,
        source,
    })?;
    if !metadata.is_dir() {
        return Err(DiscoveryError::ProjectNotDirectory {
            path: requested,
            line: entry.line,
        });
    }
    projects.entry(path_key(&canonical)).or_insert(canonical);
    Ok(())
}

fn read_projects_file(
    initial_cwd: &Path,
    file: &Path,
) -> Result<Vec<ProjectEntry>, DiscoveryError> {
    let file = if file.is_absolute() {
        file.to_path_buf()
    } else {
        initial_cwd.join(file)
    };
    let bytes = fs::read(&file).map_err(|source| DiscoveryError::ProjectsFileRead {
        path: file.clone(),
        source,
    })?;
    let text = std::str::from_utf8(&bytes).map_err(|error| {
        let line = bytes[..error.valid_up_to()]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count()
            + 1;
        DiscoveryError::ProjectsFileUtf8 {
            path: file.clone(),
            line,
        }
    })?;
    Ok(text
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line = line.trim();
            (!line.is_empty()).then(|| ProjectEntry {
                path: PathBuf::from(line),
                line: Some(index + 1),
            })
        })
        .collect())
}

fn discover_default(initial_cwd: &Path, mode: DiscoveryMode) -> Result<PathBuf, DiscoveryError> {
    let canonical = fs::canonicalize(initial_cwd).map_err(|source| DiscoveryError::InitialCwd {
        path: initial_cwd.to_path_buf(),
        source,
    })?;
    let found = walk_project_markers(&canonical);
    if let Some(config) = found.config {
        return Ok(config);
    }
    default_without_config(canonical, mode, found.git_root)
}

struct ProjectMarkers {
    config: Option<PathBuf>,
    git_root: Option<PathBuf>,
}

fn walk_project_markers(start: &Path) -> ProjectMarkers {
    let mut current = Some(start);
    let mut git_root = None;
    while let Some(directory) = current {
        if directory.join(".ahclkitconfigs").is_file() {
            return ProjectMarkers {
                config: Some(directory.to_path_buf()),
                git_root,
            };
        }
        if git_root.is_none() {
            record_git_root(&mut git_root, directory);
        }
        current = directory.parent();
    }
    ProjectMarkers {
        config: None,
        git_root,
    }
}

fn record_git_root(git_root: &mut Option<PathBuf>, directory: &Path) {
    let marker = directory.join(".git");
    if marker.is_dir() || marker.is_file() {
        *git_root = Some(directory.to_path_buf());
    }
}

fn default_without_config(
    canonical: PathBuf,
    mode: DiscoveryMode,
    git_root: Option<PathBuf>,
) -> Result<PathBuf, DiscoveryError> {
    match mode {
        DiscoveryMode::ExistingConfig => Err(DiscoveryError::ConfigNotFound { start: canonical }),
        DiscoveryMode::Initialization => Ok(git_root.unwrap_or(canonical)),
    }
}

fn path_key(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

struct ProjectEntry {
    path: PathBuf,
    line: Option<usize>,
}

#[derive(Debug)]
pub enum DiscoveryError {
    InitialCwd {
        path: PathBuf,
        source: std::io::Error,
    },
    ConfigNotFound {
        start: PathBuf,
    },
    ProjectsFileRead {
        path: PathBuf,
        source: std::io::Error,
    },
    ProjectsFileUtf8 {
        path: PathBuf,
        line: usize,
    },
    ProjectPath {
        path: PathBuf,
        line: Option<usize>,
        source: std::io::Error,
    },
    ProjectNotDirectory {
        path: PathBuf,
        line: Option<usize>,
    },
}

impl DiscoveryError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InitialCwd { .. } => "cli.initial_cwd",
            Self::ConfigNotFound { .. } => "cli.config_not_found",
            Self::ProjectsFileRead { .. } => "cli.projects_file_read",
            Self::ProjectsFileUtf8 { .. } => "cli.projects_file_utf8",
            Self::ProjectPath { .. } => "cli.project_path",
            Self::ProjectNotDirectory { .. } => "cli.project_not_directory",
        }
    }
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InitialCwd { .. } => formatter.write_str("initial current directory is invalid"),
            Self::ConfigNotFound { .. } => {
                formatter.write_str(".ahclkitconfigs was not found from the initial directory")
            }
            Self::ProjectsFileRead { .. } => formatter.write_str("projects file could not be read"),
            Self::ProjectsFileUtf8 { line, .. } => {
                write!(formatter, "projects file is not UTF-8 at line {line}")
            }
            Self::ProjectPath {
                line: Some(line), ..
            }
            | Self::ProjectNotDirectory {
                line: Some(line), ..
            } => {
                write!(
                    formatter,
                    "invalid project path at projects file line {line}"
                )
            }
            Self::ProjectPath { line: None, .. } | Self::ProjectNotDirectory { line: None, .. } => {
                formatter.write_str("invalid project path")
            }
        }
    }
}

impl Error for DiscoveryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InitialCwd { source, .. }
            | Self::ProjectsFileRead { source, .. }
            | Self::ProjectPath { source, .. } => Some(source),
            _ => None,
        }
    }
}
