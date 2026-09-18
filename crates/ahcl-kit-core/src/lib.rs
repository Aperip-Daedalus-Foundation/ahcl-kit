//! Shared domain boundaries for AHCL Kit generators and ecosystem adapters.

mod adapter;
mod diagnostic;
mod model;
mod plan;

pub use adapter::{AdapterRequest, EcosystemAdapter};
pub use diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSeverity};
pub use model::{
    CommandId, DependencyEdge, DependencyKind, InvocationContext, LicenseArtifact,
    LockfileEvidence, ProjectRoot, ProjectRootError, RepoPath, RepoPathError, ResolvedGraph,
    ResolvedPackage, UtcDate, UtcDateError,
};
pub use plan::{Change, ChangeKind, ChangePlan, PlanError};
