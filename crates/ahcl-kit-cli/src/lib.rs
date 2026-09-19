// crates/ahcl-kit-cli/src/lib.rs - Public API for CLI parsing, dispatch, and output.
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

//! Command parsing, project discovery, batch isolation, and output foundations.

mod args;
mod batch;
mod discovery;
mod dispatch;
mod output;
mod runtime;
mod runtime_impl;

pub use args::{
    InvocationError, InvocationRegistry, OutputFormat, ParsedInvocation, ProjectIdentityArgs,
};
pub use batch::{BatchExecution, BatchItem, BatchStatus, BatchValue, execute_batch};
pub use discovery::{DiscoveryError, DiscoveryMode};
pub use dispatch::run;
pub use output::{
    AggregateCounts, CommandReport, OutputChange, OutputChangeKind, OutputDiagnostic,
    OutputSeverity, ProjectReport, ProjectStatus, not_wired_report, render_json, render_text,
};
pub use runtime::{
    AdapterKind, CommandRuntime, LanguageAdapterRegistry, PlanRequest, PlanScope, ResolvedAdapter,
    RuntimeError, RuntimePlan, UnavailableRuntime,
};
pub use runtime_impl::{ConcreteRuntime, system_utc_date};
