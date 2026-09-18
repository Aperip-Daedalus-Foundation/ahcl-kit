use crate::{ProjectRoot, RepoPath, ResolvedGraph};
use std::error::Error;

/// The normalized input an ecosystem adapter needs to resolve a project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterRequest {
    project_root: ProjectRoot,
    manifest_paths: Vec<RepoPath>,
}

impl AdapterRequest {
    pub fn new(project_root: ProjectRoot, manifest_paths: Vec<RepoPath>) -> Self {
        Self {
            project_root,
            manifest_paths,
        }
    }

    pub fn project_root(&self) -> &ProjectRoot {
        &self.project_root
    }

    pub fn manifest_paths(&self) -> &[RepoPath] {
        &self.manifest_paths
    }
}

/// Resolves ecosystem-specific metadata into the core's normalized graph.
///
/// The trait deliberately exposes no Cargo types, generic methods, or `Self`
/// returns so it can be stored behind `dyn EcosystemAdapter`.
pub trait EcosystemAdapter: Send + Sync {
    fn ecosystem(&self) -> &'static str;

    fn resolve(
        &self,
        request: &AdapterRequest,
    ) -> Result<ResolvedGraph, Box<dyn Error + Send + Sync>>;
}
