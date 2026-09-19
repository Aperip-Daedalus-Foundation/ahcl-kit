// crates/ahcl-kit-config/src/ast.rs - Configuration syntax tree types.
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

use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScalarValue {
    String(String),
    Boolean(bool),
    Integer(i64),
}

impl ScalarValue {
    pub(crate) fn render(&self) -> String {
        match self {
            Self::String(value) => format!("\"{}\"", escape_string(value)),
            Self::Boolean(value) => value.to_string(),
            Self::Integer(value) => value.to_string(),
        }
    }
}

fn escape_string(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Value {
    Scalar(ScalarValue),
    EmptyList,
    ScalarList(Vec<ScalarValue>),
    ObjectList(Vec<BTreeMap<String, ScalarValue>>),
}

impl Value {
    pub(crate) fn is_scalar(&self) -> bool {
        matches!(self, Self::Scalar(_))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Assignment {
    pub(crate) section: Option<String>,
    pub(crate) key: String,
    pub(crate) value: Value,
    pub(crate) value_start: usize,
    pub(crate) value_end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Section {
    pub(crate) name: String,
    pub(crate) insertion_offset: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DocumentAst {
    pub(crate) assignments: Vec<Assignment>,
    pub(crate) section_order: Vec<String>,
    pub(crate) sections: Vec<Section>,
    pub(crate) root_insertion_offset: usize,
}

impl DocumentAst {
    pub(crate) fn assignment(&self, section: Option<&str>, key: &str) -> Option<&Assignment> {
        self.assignments
            .iter()
            .find(|assignment| assignment.section.as_deref() == section && assignment.key == key)
    }

    pub(crate) fn section_insertion_offset(&self, section: &str) -> Option<usize> {
        self.sections
            .iter()
            .find(|entry| entry.name == section)
            .map(|entry| entry.insertion_offset)
    }
}
