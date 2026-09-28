// crates/ahcl-kit-cargo/src/lib.rs - Public API for Cargo dependency resolution.
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

//! Cargo dependency resolution and byte-preserving license evidence collection.

mod adapter;
mod collector;
mod components;
mod graph;
mod host;
mod limits;
mod settings;
mod upstream;
mod upstream_manifest;
mod upstream_url;

pub use adapter::{CargoAdapter, CargoError, CargoResolveRequest};
pub use ahcl_kit_core::{PackageDirectoryInput, assign_package_directories};
pub use components::{CargoComponentError, CargoComponentResolution};
pub use host::HOST;
pub use limits::EvidenceLimits;
pub use settings::CONTRIBUTOR;
pub use upstream::{
    CargoEvidenceRequest, CargoEvidenceResponse, CargoEvidenceTransport, CargoTransportError,
};
