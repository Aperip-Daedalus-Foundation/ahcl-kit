// crates/ahcl-kit-materials/src/platform_fs.rs - Platform filesystem backend selection and limits.
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

use crate::MaterialsError;

pub(crate) const MAX_MANAGED_ROOT_ENTRIES: usize = 2_048;
pub(crate) const MAX_MANAGED_PACKAGES: usize = 1_024;
pub(crate) const MAX_MANAGED_EVIDENCE_PER_PACKAGE: usize = 64;
pub(crate) const MAX_MANAGED_TOTAL_ENTRIES: usize = 8_192;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(crate) use unix::{ManagedDirectory, PlatformRoot};
#[cfg(windows)]
pub(crate) use windows::{ManagedDirectory, PlatformRoot};

fn inventory_limit_error() -> MaterialsError {
    MaterialsError::filesystem(
        "materials.managed.inventory_limit",
        "managed third-party inventory exceeds supported entry limits",
    )
}
