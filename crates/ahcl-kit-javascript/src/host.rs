// crates/ahcl-kit-javascript/src/host.rs - JavaScript resolution host.
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

use crate::adapter::{JavascriptAdapter, JavascriptResolveRequest};
use ahcl_kit_config::{ComponentResolution, EffectiveConfig, HostFailure, LanguageHost};
use ahcl_kit_core::{ProjectRoot, ResolvedGraph};

pub static HOST: JavascriptHost = JavascriptHost;

#[derive(Clone, Copy, Debug)]
pub struct JavascriptHost;

impl LanguageHost for JavascriptHost {
    fn language_id(&self) -> &'static str {
        "javascript"
    }

    fn resolve(
        &self,
        project: &ProjectRoot,
        config: &EffectiveConfig,
    ) -> Result<ResolvedGraph, HostFailure> {
        let request = JavascriptResolveRequest::from_config(project.clone(), config);
        JavascriptAdapter::new()
            .resolve_request(&request)
            .map_err(|error| HostFailure::new(error.code(), error.to_string()))
    }

    fn resolve_components(
        &self,
        _project: &ProjectRoot,
        _config: &EffectiveConfig,
    ) -> Result<Vec<ComponentResolution>, HostFailure> {
        Ok(Vec::new())
    }
}
