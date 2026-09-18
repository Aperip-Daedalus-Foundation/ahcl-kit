use crate::{ProjectRoot, RepoPath};

/// A stable, machine-readable diagnostic code.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DiagnosticCode(String);

impl DiagnosticCode {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The presentation severity of a diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
}

/// A sanitized domain diagnostic tied to an optional project and path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub project_root: Option<ProjectRoot>,
    pub path: Option<RepoPath>,
}

impl Diagnostic {
    pub fn new(
        code: DiagnosticCode,
        severity: DiagnosticSeverity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity,
            message: message.into(),
            project_root: None,
            path: None,
        }
    }

    pub fn for_project(mut self, project_root: ProjectRoot) -> Self {
        self.project_root = Some(project_root);
        self
    }

    pub fn at_path(mut self, path: RepoPath) -> Self {
        self.path = Some(path);
        self
    }
}
