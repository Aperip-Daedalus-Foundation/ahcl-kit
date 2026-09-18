use crate::graph;
use crate::limits::EvidenceLimits;
use ahcl_kit_config::{CargoRuleClassification, CargoSettings, EffectiveConfig};
use ahcl_kit_core::{AdapterRequest, EcosystemAdapter, ProjectRoot, RepoPath, ResolvedGraph};
use std::error::Error;
use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CargoResolveRequest {
    project_root: ProjectRoot,
    manifests: Vec<RepoPath>,
    packages: Vec<String>,
    settings: Option<CargoSettings>,
    strict_license_files: bool,
    limits: EvidenceLimits,
}

impl CargoResolveRequest {
    pub fn from_config(
        project_root: ProjectRoot,
        config: &EffectiveConfig,
        strict_license_files: bool,
    ) -> Self {
        let settings = config.rust().cargo().clone();
        Self {
            project_root,
            manifests: settings.manifests().to_vec(),
            packages: settings.packages().to_vec(),
            settings: Some(settings),
            strict_license_files,
            limits: EvidenceLimits::default(),
        }
    }

    pub fn from_adapter_request(request: &AdapterRequest) -> Self {
        Self {
            project_root: request.project_root().clone(),
            manifests: request.manifest_paths().to_vec(),
            packages: Vec::new(),
            settings: None,
            strict_license_files: true,
            limits: EvidenceLimits::default(),
        }
    }

    pub fn project_root(&self) -> &ProjectRoot {
        &self.project_root
    }

    pub fn manifests(&self) -> &[RepoPath] {
        &self.manifests
    }

    pub fn packages(&self) -> &[String] {
        &self.packages
    }

    pub fn strict_license_files(&self) -> bool {
        self.strict_license_files
    }

    pub fn limits(&self) -> EvidenceLimits {
        self.limits
    }

    pub fn with_limits(mut self, limits: EvidenceLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn push_manifest(&mut self, manifest: RepoPath) {
        self.manifests.push(manifest);
    }

    pub(crate) fn classify(&self, package: &str, source: &str) -> CargoRuleClassification {
        self.settings
            .as_ref()
            .map_or(CargoRuleClassification::ThirdParty, |settings| {
                settings.classify(package, source)
            })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CargoAdapter;

impl CargoAdapter {
    pub const fn new() -> Self {
        Self
    }

    pub fn resolve_request(
        &self,
        request: &CargoResolveRequest,
    ) -> Result<ResolvedGraph, CargoError> {
        graph::resolve(request)
    }
}

impl EcosystemAdapter for CargoAdapter {
    fn ecosystem(&self) -> &'static str {
        "cargo"
    }

    fn resolve(
        &self,
        request: &AdapterRequest,
    ) -> Result<ResolvedGraph, Box<dyn Error + Send + Sync>> {
        self.resolve_request(&CargoResolveRequest::from_adapter_request(request))
            .map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>)
    }
}

#[derive(Debug)]
pub enum CargoError {
    Metadata {
        manifest: RepoPath,
        message: String,
    },
    MissingResolve {
        manifest: RepoPath,
    },
    PackageSelection {
        package: String,
    },
    MissingPackage {
        package_id: String,
    },
    MissingNode {
        package_id: String,
    },
    LockfileOutsideProject,
    LockfileRead {
        path: PathBuf,
        source: std::io::Error,
    },
    InvalidRepositoryPath {
        path: String,
    },
    EvidenceRead {
        package_id: String,
        source: std::io::Error,
    },
    EvidencePathEncoding {
        package_id: String,
    },
    EvidenceOutsidePackage {
        package_id: String,
    },
    LinkOrReparsePoint {
        package_id: String,
    },
    EvidenceFileTooLarge {
        package_id: String,
        byte_len: u64,
    },
    TooManyEvidenceFiles {
        package_id: String,
        count: u64,
    },
    AggregateEvidenceTooLarge {
        byte_len: u64,
    },
    MissingLicenseEvidence {
        package_id: String,
    },
}

impl CargoError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Metadata { .. } => "cargo.metadata_failed",
            Self::MissingResolve { .. } => "cargo.resolve_missing",
            Self::PackageSelection { .. } => "cargo.package_selection",
            Self::MissingPackage { .. } | Self::MissingNode { .. } => "cargo.metadata_incomplete",
            Self::LockfileOutsideProject => "cargo.lockfile_outside_project",
            Self::LockfileRead { .. } => "cargo.lockfile_read",
            Self::InvalidRepositoryPath { .. } => "cargo.path_invalid",
            Self::EvidenceRead { .. } => "cargo.evidence_read",
            Self::EvidencePathEncoding { .. } => "cargo.evidence_path_encoding",
            Self::EvidenceOutsidePackage { .. } => "cargo.evidence_outside_package",
            Self::LinkOrReparsePoint { .. } => "cargo.evidence_link",
            Self::EvidenceFileTooLarge { .. } => "cargo.evidence_file_too_large",
            Self::TooManyEvidenceFiles { .. } => "cargo.evidence_file_count",
            Self::AggregateEvidenceTooLarge { .. } => "cargo.evidence_aggregate_too_large",
            Self::MissingLicenseEvidence { .. } => "cargo.evidence_missing",
        }
    }
}

impl fmt::Display for CargoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata { manifest, .. } => {
                write!(formatter, "Cargo metadata failed for {manifest}")
            }
            Self::MissingResolve { manifest } => write!(
                formatter,
                "Cargo metadata omitted the resolve graph for {manifest}"
            ),
            Self::PackageSelection { package } => write!(
                formatter,
                "Cargo workspace package selection did not match: {package}"
            ),
            Self::MissingPackage { package_id } => {
                write!(formatter, "Cargo metadata omitted package {package_id}")
            }
            Self::MissingNode { package_id } => write!(
                formatter,
                "Cargo metadata omitted resolve node {package_id}"
            ),
            Self::LockfileOutsideProject => {
                formatter.write_str("Cargo lockfile is outside the project root")
            }
            Self::LockfileRead { .. } => formatter.write_str("Cargo lockfile could not be read"),
            Self::InvalidRepositoryPath { .. } => {
                formatter.write_str("Cargo path cannot be represented safely")
            }
            Self::EvidenceRead { .. } => {
                formatter.write_str("Cargo license evidence could not be read")
            }
            Self::EvidencePathEncoding { .. } => {
                formatter.write_str("Cargo license evidence path is not Unicode")
            }
            Self::EvidenceOutsidePackage { .. } => {
                formatter.write_str("Cargo license evidence escapes the package root")
            }
            Self::LinkOrReparsePoint { .. } => {
                formatter.write_str("Cargo license evidence uses a link or reparse point")
            }
            Self::EvidenceFileTooLarge { .. } => {
                formatter.write_str("Cargo license evidence exceeds the per-file limit")
            }
            Self::TooManyEvidenceFiles { .. } => {
                formatter.write_str("Cargo package exceeds the evidence-file count limit")
            }
            Self::AggregateEvidenceTooLarge { .. } => {
                formatter.write_str("Cargo license evidence exceeds the aggregate limit")
            }
            Self::MissingLicenseEvidence { .. } => {
                formatter.write_str("Cargo package has no license evidence")
            }
        }
    }
}

impl Error for CargoError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::LockfileRead { source, .. } | Self::EvidenceRead { source, .. } => Some(source),
            _ => None,
        }
    }
}
