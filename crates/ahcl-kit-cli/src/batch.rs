// crates/ahcl-kit-cli/src/batch.rs - Deterministic batch execution and status aggregation.
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

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatchStatus {
    Success,
    Drift,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchValue<T> {
    pub status: BatchStatus,
    pub value: T,
}

impl<T> BatchValue<T> {
    pub fn success(value: T) -> Self {
        Self {
            status: BatchStatus::Success,
            value,
        }
    }

    pub fn drift(value: T) -> Self {
        Self {
            status: BatchStatus::Drift,
            value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchItem<T, E> {
    pub project: PathBuf,
    pub result: Result<BatchValue<T>, E>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchExecution<T, E> {
    pub items: Vec<BatchItem<T, E>>,
}

impl<T, E> BatchExecution<T, E> {
    pub fn exit_code(&self) -> u8 {
        if self.items.iter().any(|item| item.result.is_err()) {
            1
        } else if self.items.iter().any(|item| {
            item.result
                .as_ref()
                .is_ok_and(|value| value.status == BatchStatus::Drift)
        }) {
            2
        } else {
            0
        }
    }
}

pub fn execute_batch<T, E, F>(
    mut projects: Vec<PathBuf>,
    fail_fast: bool,
    mut operation: F,
) -> BatchExecution<T, E>
where
    F: FnMut(&Path) -> Result<BatchValue<T>, E>,
{
    projects.sort_by_key(|project| path_key(project));
    let mut items = Vec::new();
    for project in projects {
        let result = operation(&project);
        let failed = result.is_err();
        items.push(BatchItem { project, result });
        if failed && fail_fast {
            break;
        }
    }
    BatchExecution { items }
}

fn path_key(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        normalized.to_lowercase()
    } else {
        normalized
    }
}
