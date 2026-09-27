// crates/ahcl-kit-cargo/src/host.rs - Cargo resolution host.
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

use crate::adapter::{CargoAdapter, CargoResolveRequest};
use crate::components::CargoComponentResolution;
use crate::limits::EvidenceLimits;
use crate::settings::{CargoBinding, ComponentLayout};
use ahcl_kit_config::{ComponentResolution, EffectiveConfig, HostFailure, LanguageHost};
use ahcl_kit_core::{ProjectRoot, ResolvedGraph};

pub static HOST: CargoHost = CargoHost;

#[derive(Clone, Copy, Debug)]
pub struct CargoHost;

impl LanguageHost for CargoHost {
    fn language_id(&self) -> &'static str {
        "rust"
    }

    fn resolve(
        &self,
        project: &ProjectRoot,
        config: &EffectiveConfig,
    ) -> Result<ResolvedGraph, HostFailure> {
        let limits = config.limits();
        let request = CargoResolveRequest::from_config(
            project.clone(),
            config,
            config.generation().strict_license_files(),
        )
        .with_limits(EvidenceLimits::new(
            limits.evidence_file_bytes(),
            limits.files_per_package(),
            limits.aggregate_evidence_bytes(),
        ));
        CargoAdapter::new()
            .resolve_request(&request)
            .map_err(|error| HostFailure::new(error.code(), error.to_string()))
    }

    fn resolve_components(
        &self,
        project: &ProjectRoot,
        config: &EffectiveConfig,
    ) -> Result<Vec<ComponentResolution>, HostFailure> {
        let Some(binding) = CargoBinding::from_config(config) else {
            return Ok(Vec::new());
        };
        let mut resolved = Vec::new();
        for component in binding.settings().components() {
            if !component.enabled() {
                continue;
            }
            let resolution = CargoAdapter::new()
                .resolve_component(project, config, component)
                .map_err(|error| HostFailure::new(error.code(), error.to_string()))?;
            resolved.push(component_resolution(resolution));
        }
        Ok(resolved)
    }
}

fn component_resolution(resolution: CargoComponentResolution) -> ComponentResolution {
    let centralized = resolution.component().layout() == ComponentLayout::Centralized;
    ComponentResolution::new(
        resolution.config().clone(),
        resolution.graph().clone(),
        resolution.component_root().cloned(),
        centralized,
    )
}
