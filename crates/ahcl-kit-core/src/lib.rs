// crates/ahcl-kit-core/src/lib.rs - Public API for shared domain boundaries.
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
pub use plan::{
    Change, ChangeKind, ChangePlan, PlanError, ProjectEntry, ProjectView, ProjectViewError,
};
