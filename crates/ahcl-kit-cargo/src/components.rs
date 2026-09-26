// crates/ahcl-kit-cargo/src/components.rs - Cargo component resolution.
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

use crate::{CargoAdapter, CargoError, CargoResolveRequest, EvidenceLimits};
use ahcl_kit_config::{CargoComponent, EffectiveConfig};
use ahcl_kit_core::{ProjectRoot, RepoPath, ResolvedGraph};
use std::fmt;

/// A Cargo component resolved from one explicitly configured workspace package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CargoComponentResolution {
    component: CargoComponent,
    config: EffectiveConfig,
    package_id: String,
    component_root: Option<RepoPath>,
    graph: ResolvedGraph,
}

impl CargoComponentResolution {
    pub(crate) fn new(
        component: CargoComponent,
        config: EffectiveConfig,
        package_id: String,
        component_root: Option<RepoPath>,
        graph: ResolvedGraph,
    ) -> Self {
        Self {
            component,
            config,
            package_id,
            component_root,
            graph,
        }
    }

    pub fn component(&self) -> &CargoComponent {
        &self.component
    }

    pub fn config(&self) -> &EffectiveConfig {
        &self.config
    }

    pub fn package_id(&self) -> &str {
        &self.package_id
    }

    pub fn component_root(&self) -> Option<&RepoPath> {
        self.component_root.as_ref()
    }

    pub fn graph(&self) -> &ResolvedGraph {
        &self.graph
    }
}

#[derive(Debug)]
pub enum CargoComponentError {
    InvalidConfig {
        component: String,
        source: ahcl_kit_config::ConfigError,
    },
    PackageMissing {
        component: String,
        package: String,
    },
    PackageAmbiguous {
        component: String,
        package: String,
    },
    PackagePathOutsideProject {
        component: String,
        path: String,
    },
    UnsafePackagePath {
        component: String,
        path: String,
    },
    Resolve {
        component: String,
        source: Box<CargoError>,
    },
}

impl CargoComponentError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidConfig { .. } => "cargo.component_config",
            Self::PackageMissing { .. } | Self::PackageAmbiguous { .. } => {
                "cargo.component_package"
            }
            Self::PackagePathOutsideProject { .. } => "cargo.component_path",
            Self::UnsafePackagePath { .. } => "cargo.component_path",
            Self::Resolve { source, .. } => source.code(),
        }
    }
}

impl fmt::Display for CargoComponentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig { component, source } => {
                write!(
                    formatter,
                    "Cargo component {component} configuration is invalid: {source}"
                )
            }
            Self::PackageMissing { component, package } => {
                write!(
                    formatter,
                    "Cargo component {component} package {package} was not resolved"
                )
            }
            Self::PackageAmbiguous { component, package } => {
                write!(
                    formatter,
                    "Cargo component {component} package selection is ambiguous: {package}"
                )
            }
            Self::PackagePathOutsideProject { component, path } => {
                write!(
                    formatter,
                    "Cargo component {component} manifest is outside the project: {path}"
                )
            }
            Self::UnsafePackagePath { component, path } => {
                write!(
                    formatter,
                    "Cargo component {component} manifest path is not repository-safe: {path}"
                )
            }
            Self::Resolve { component, source } => {
                write!(
                    formatter,
                    "Cargo component {component} resolution failed: {source}"
                )
            }
        }
    }
}

impl std::error::Error for CargoComponentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidConfig { source, .. } => Some(source),
            Self::Resolve { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl CargoAdapter {
    pub fn resolve_component(
        &self,
        project_root: &ProjectRoot,
        config: &EffectiveConfig,
        component: &CargoComponent,
    ) -> Result<CargoComponentResolution, CargoComponentError> {
        if !component.enabled() {
            return Err(CargoComponentError::InvalidConfig {
                component: component.id().to_owned(),
                source: ahcl_kit_config::ConfigError::InvalidValue {
                    path: format!("rust.cargo.component.{}.enabled", component.id()),
                    message: "disabled components are not resolved".to_owned(),
                },
            });
        }
        let component_config = config.for_cargo_component(component).map_err(|source| {
            CargoComponentError::InvalidConfig {
                component: component.id().to_owned(),
                source,
            }
        })?;
        let request = CargoResolveRequest::from_config(
            project_root.clone(),
            &component_config,
            component_config.generation().strict_license_files(),
        )
        .with_limits(EvidenceLimits::new(
            component_config.limits().evidence_file_bytes(),
            component_config.limits().files_per_package(),
            component_config.limits().aggregate_evidence_bytes(),
        ));
        let graph =
            self.resolve_request(&request)
                .map_err(|source| CargoComponentError::Resolve {
                    component: component.id().to_owned(),
                    source: Box::new(source),
                })?;
        let mut selected = graph
            .packages
            .iter()
            .filter(|package| package.first_party && package.name == component.package())
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return Err(CargoComponentError::PackageMissing {
                component: component.id().to_owned(),
                package: component.package().to_owned(),
            });
        }
        if selected.len() > 1 {
            return Err(CargoComponentError::PackageAmbiguous {
                component: component.id().to_owned(),
                package: component.package().to_owned(),
            });
        }
        let package = selected.remove(0);
        let manifest = package.manifest_path.as_path();
        let root =
            manifest
                .parent()
                .ok_or_else(|| CargoComponentError::PackagePathOutsideProject {
                    component: component.id().to_owned(),
                    path: manifest.display().to_string(),
                })?;
        let relative = root.strip_prefix(project_root.as_path()).map_err(|_| {
            CargoComponentError::PackagePathOutsideProject {
                component: component.id().to_owned(),
                path: root.display().to_string(),
            }
        })?;
        let relative = relative
            .to_str()
            .ok_or_else(|| CargoComponentError::UnsafePackagePath {
                component: component.id().to_owned(),
                path: root.display().to_string(),
            })?;
        let component_root = if relative.is_empty() {
            None
        } else {
            Some(RepoPath::parse(relative.replace('\\', "/")).map_err(|_| {
                CargoComponentError::UnsafePackagePath {
                    component: component.id().to_owned(),
                    path: relative.to_owned(),
                }
            })?)
        };
        Ok(CargoComponentResolution::new(
            component.clone(),
            component_config,
            package.id.clone(),
            component_root,
            graph,
        ))
    }
}
