// crates/ahcl-kit-core/src/adapter.rs - Ecosystem adapter interfaces.
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
