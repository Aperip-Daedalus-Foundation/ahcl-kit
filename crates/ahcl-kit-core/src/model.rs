// crates/ahcl-kit-core/src/model.rs - Core project and dependency data models.
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

use std::fmt;
use std::path::{Path, PathBuf};

/// An absolute path selected as the boundary for one project operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectRoot(PathBuf);

impl ProjectRoot {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, ProjectRootError> {
        let path = path.into();
        if !path.is_absolute() {
            return Err(ProjectRootError::NotAbsolute);
        }
        if path.parent().is_none() {
            return Err(ProjectRootError::FilesystemRoot);
        }
        Ok(Self(path))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn resolve(&self, path: &RepoPath) -> PathBuf {
        self.0.join(path.as_path())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectRootError {
    NotAbsolute,
    FilesystemRoot,
}

impl fmt::Display for ProjectRootError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAbsolute => formatter.write_str("project root must be absolute"),
            Self::FilesystemRoot => formatter.write_str("filesystem root cannot be a project root"),
        }
    }
}

impl std::error::Error for ProjectRootError {}

/// A normalized, non-empty path inside a project root.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepoPath(String);

impl RepoPath {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, RepoPathError> {
        let value = value.as_ref();
        if value.is_empty() {
            return Err(RepoPathError::Empty);
        }
        if value.starts_with('/') || value.starts_with('\\') || has_windows_prefix(value) {
            return Err(RepoPathError::AbsoluteOrPrefixed);
        }

        let mut parts = Vec::new();
        for part in value.split(['/', '\\']) {
            if part.is_empty() || part == "." {
                return Err(RepoPathError::EmptyComponent);
            }
            if part == ".." {
                return Err(RepoPathError::ParentTraversal);
            }
            if !is_safe_component(part) {
                return Err(RepoPathError::ReservedComponent(part.to_owned()));
            }
            parts.push(part);
        }

        Ok(Self(parts.join("/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl fmt::Display for RepoPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepoPathError {
    Empty,
    EmptyComponent,
    ParentTraversal,
    AbsoluteOrPrefixed,
    ReservedComponent(String),
}

impl fmt::Display for RepoPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("repository-relative path cannot be empty"),
            Self::EmptyComponent => {
                formatter.write_str("repository-relative path has an empty component")
            }
            Self::ParentTraversal => {
                formatter.write_str("repository-relative path cannot traverse parents")
            }
            Self::AbsoluteOrPrefixed => {
                formatter.write_str("repository-relative path cannot be absolute or prefixed")
            }
            Self::ReservedComponent(component) => {
                write!(
                    formatter,
                    "repository-relative path has an unsafe component: {component}"
                )
            }
        }
    }
}

impl std::error::Error for RepoPathError {}

fn has_windows_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn is_safe_component(component: &str) -> bool {
    if component.ends_with([' ', '.'])
        || component.chars().any(|character| {
            character.is_control() || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
        })
    {
        return false;
    }

    let base_name = component
        .split_once('.')
        .map_or(component, |(name, _)| name)
        .to_ascii_uppercase();
    if matches!(base_name.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return false;
    }

    let Some(number) = base_name
        .strip_prefix("COM")
        .or_else(|| base_name.strip_prefix("LPT"))
    else {
        return true;
    };

    !matches!(
        number,
        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
    )
}

/// A stable business identifier independent from the executable name.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum CommandId {
    ConfigInit,
    ConfigValidate,
    ConfigShowResolved,
    ProjectInit,
    ProjectGenerate,
    ProjectCheck,
    LicenseSync,
    DependencyGenerate,
    ThirdPartyGenerate,
}

impl CommandId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConfigInit => "config.init",
            Self::ConfigValidate => "config.validate",
            Self::ConfigShowResolved => "config.show-resolved",
            Self::ProjectInit => "project.init",
            Self::ProjectGenerate => "project.generate",
            Self::ProjectCheck => "project.check",
            Self::LicenseSync => "license.sync",
            Self::DependencyGenerate => "dependency.generate",
            Self::ThirdPartyGenerate => "third-party.generate",
        }
    }
}

/// Process-independent inputs captured at the command boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvocationContext {
    command_id: CommandId,
    invocation_name: String,
    current_utc_date: UtcDate,
}

impl InvocationContext {
    pub fn new(
        command_id: CommandId,
        invocation_name: impl Into<String>,
        current_utc_date: UtcDate,
    ) -> Self {
        Self {
            command_id,
            invocation_name: invocation_name.into(),
            current_utc_date,
        }
    }

    pub fn command_id(&self) -> CommandId {
        self.command_id
    }

    pub fn invocation_name(&self) -> &str {
        &self.invocation_name
    }

    pub fn current_utc_date(&self) -> UtcDate {
        self.current_utc_date
    }
}

/// A caller-supplied UTC calendar date used for reproducible generation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct UtcDate {
    year: u16,
    month: u8,
    day: u8,
}

impl UtcDate {
    pub fn new(year: u16, month: u8, day: u8) -> Result<Self, UtcDateError> {
        if year == 0 || month == 0 || month > 12 || day == 0 || day > days_in_month(year, month) {
            return Err(UtcDateError::InvalidCalendarDate { year, month, day });
        }
        Ok(Self { year, month, day })
    }

    pub fn year(self) -> u16 {
        self.year
    }

    pub fn month(self) -> u8 {
        self.month
    }

    pub fn day(self) -> u8 {
        self.day
    }
}

impl fmt::Display for UtcDate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.year, self.month, self.day
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UtcDateError {
    InvalidCalendarDate { year: u16, month: u8, day: u8 },
}

impl fmt::Display for UtcDateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCalendarDate { year, month, day } => {
                write!(formatter, "invalid UTC date: {year:04}-{month:02}-{day:02}")
            }
        }
    }
}

impl std::error::Error for UtcDateError {}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Adapter-normalized dependency information.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ResolvedGraph {
    pub packages: Vec<ResolvedPackage>,
    pub edges: Vec<DependencyEdge>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedPackage {
    pub id: String,
    pub name: String,
    pub version: String,
    pub source: Option<String>,
    pub checksum: Option<String>,
    pub manifest_path: PathBuf,
    pub repository: Option<String>,
    pub homepage: Option<String>,
    pub authors: Vec<String>,
    pub declared_license: Option<String>,
    pub first_party: bool,
    pub contributing_lockfiles: Vec<LockfileEvidence>,
    pub license_artifacts: Vec<LicenseArtifact>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyEdge {
    pub from_package_id: String,
    pub to_package_id: String,
    pub kind: DependencyKind,
    pub target_conditions: Vec<String>,
    pub direct: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DependencyKind {
    Normal,
    Build,
    Development,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LockfileEvidence {
    pub path: RepoPath,
    pub sha256: String,
    pub byte_len: u64,
}

/// A discovered license file preserved exactly as raw bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LicenseArtifact {
    pub relative_path: RepoPath,
    pub bytes: Vec<u8>,
}
