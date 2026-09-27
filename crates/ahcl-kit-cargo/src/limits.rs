// crates/ahcl-kit-cargo/src/limits.rs - Evidence limits, hashing, and package directory assignment.
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

pub const MAX_EVIDENCE_FILE_BYTES: u64 = 2_097_152;
pub const MAX_EVIDENCE_FILES_PER_PACKAGE: u64 = 64;
pub const MAX_AGGREGATE_EVIDENCE_BYTES: u64 = 536_870_912;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EvidenceLimits {
    max_file_bytes: u64,
    max_files_per_package: u64,
    max_aggregate_bytes: u64,
}

impl EvidenceLimits {
    pub const fn new(
        max_file_bytes: u64,
        max_files_per_package: u64,
        max_aggregate_bytes: u64,
    ) -> Self {
        Self {
            max_file_bytes,
            max_files_per_package,
            max_aggregate_bytes,
        }
    }

    pub const fn max_file_bytes(self) -> u64 {
        self.max_file_bytes
    }

    pub const fn max_files_per_package(self) -> u64 {
        self.max_files_per_package
    }

    pub const fn max_aggregate_bytes(self) -> u64 {
        self.max_aggregate_bytes
    }
}

impl Default for EvidenceLimits {
    fn default() -> Self {
        Self::new(
            MAX_EVIDENCE_FILE_BYTES,
            MAX_EVIDENCE_FILES_PER_PACKAGE,
            MAX_AGGREGATE_EVIDENCE_BYTES,
        )
    }
}
