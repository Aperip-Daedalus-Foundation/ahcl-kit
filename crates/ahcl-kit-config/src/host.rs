// crates/ahcl-kit-config/src/host.rs - Language-neutral resolution host contract.
//
// Copyright (C) 2026 Aperip Daedalus Foundation. All rights reserved.
//
// The AHCL-covered material identified below forms part of
// AHCL Kit and is provided under version 1.2 of the
// Aperip Heimdall Commons License (AHCL). The applicable version is also subject
// to the AHCL provisions concerning Continuous AHCL Licensing Segments and
// migration to later official versions.
//
// AHCL-covered portions: the whole file
//
// Subject to Section 3.1 of AHCL, after having a reasonable opportunity to
// read AHCL, all applicable Additional Restrictions, and all version notices,
// a person accepts the corresponding terms by engaging in Use of the
// AHCL-covered material identified above. Any separate or affirmative assent
// required by applicable mandatory law must still be obtained.
//
// Official AHCL text and public notices:          https://ahcl.aperip.com
// Applicable LICENSE:                             LICENSE
// Paths below are relative to the directory containing that LICENSE.
// AHCL Materials Directory:                       .ahcl/
// Repository official or recognized AHCL copy:   .ahcl/AHCL-1.2.md
// Project canonical repository:                   https://github.com/Aperip-Daedalus-Foundation/ahcl-kit
// AHCL origin and project notice:                 .ahcl/AHCL-PROJECT-NOTICE.md
// AHCL Version Adoption records:                  .ahcl/AHCL-VERSION-ADOPTION.md
// Complete Corresponding Source and history:      .ahcl/AHCL-SOURCE.md
// Dependencies, Referenced Materials, and licenses:
//                                                    .ahcl/AHCL-DEPENDENCIES.md
//
// SPDX-License-Identifier: LicenseRef-AHCL-1.2

use crate::binding::LanguageContributor;
use crate::schema::EffectiveConfig;
use ahcl_kit_core::{ProjectRoot, RepoPath, ResolvedGraph};
use std::error::Error;
use std::fmt;

/// Contributors and hosts supplied by the composition root.
///
/// Public crates never name an ecosystem. The root that links extension crates
/// passes this value into configuration parsing and command execution.
#[derive(Clone, Copy)]
pub struct LanguageInstallation {
    contributors: &'static [&'static dyn LanguageContributor],
    hosts: &'static [&'static dyn LanguageHost],
}

impl LanguageInstallation {
    pub const fn new(
        contributors: &'static [&'static dyn LanguageContributor],
        hosts: &'static [&'static dyn LanguageHost],
    ) -> Self {
        Self {
            contributors,
            hosts,
        }
    }

    pub const fn contributors(&self) -> &'static [&'static dyn LanguageContributor] {
        self.contributors
    }

    pub const fn hosts(&self) -> &'static [&'static dyn LanguageHost] {
        self.hosts
    }
}

/// One installed ecosystem's project and component resolution.
pub trait LanguageHost: Send + Sync {
    fn language_id(&self) -> &'static str;

    fn resolve(
        &self,
        project: &ProjectRoot,
        config: &EffectiveConfig,
    ) -> Result<ResolvedGraph, HostFailure>;

    fn resolve_components(
        &self,
        project: &ProjectRoot,
        config: &EffectiveConfig,
    ) -> Result<Vec<ComponentResolution>, HostFailure>;
}

/// A component narrowed from one language binding, without naming that language.
#[derive(Clone, Debug)]
pub struct ComponentResolution {
    config: EffectiveConfig,
    graph: ResolvedGraph,
    component_root: Option<RepoPath>,
    centralized: bool,
}

impl ComponentResolution {
    pub fn new(
        config: EffectiveConfig,
        graph: ResolvedGraph,
        component_root: Option<RepoPath>,
        centralized: bool,
    ) -> Self {
        Self {
            config,
            graph,
            component_root,
            centralized,
        }
    }

    pub fn config(&self) -> &EffectiveConfig {
        &self.config
    }

    pub fn graph(&self) -> &ResolvedGraph {
        &self.graph
    }

    pub fn component_root(&self) -> Option<&RepoPath> {
        self.component_root.as_ref()
    }

    pub fn centralized(&self) -> bool {
        self.centralized
    }
}

/// Ecosystem failure reported without exposing the ecosystem error type.
#[derive(Debug)]
pub struct HostFailure {
    code: &'static str,
    message: String,
}

impl HostFailure {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for HostFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for HostFailure {}
