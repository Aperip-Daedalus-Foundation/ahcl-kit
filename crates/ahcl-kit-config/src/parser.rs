// crates/ahcl-kit-config/src/parser.rs - Strict configuration parser.
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

use crate::ast::{Assignment, DocumentAst, ScalarValue, Section, Value};
use crate::binding::LanguageContributor;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const MAX_CONFIG_BYTES: usize = 1_048_576;

#[derive(Clone)]
pub struct ConfigDocument {
    source: String,
    ast: DocumentAst,
    contributors: Vec<&'static dyn LanguageContributor>,
}

impl ConfigDocument {
    pub fn parse(source: &str) -> Result<Self, ConfigError> {
        Self::parse_with(source, &[])
    }

    pub fn parse_bytes(bytes: &[u8]) -> Result<Self, ConfigError> {
        Self::parse_bytes_with(bytes, &[])
    }

    pub fn parse_with(
        source: &str,
        contributors: &[&'static dyn LanguageContributor],
    ) -> Result<Self, ConfigError> {
        Self::parse_bytes_with(source.as_bytes(), contributors)
    }

    pub fn parse_bytes_with(
        bytes: &[u8],
        contributors: &[&'static dyn LanguageContributor],
    ) -> Result<Self, ConfigError> {
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(ConfigError::FileTooLarge { bytes: bytes.len() });
        }
        let source = std::str::from_utf8(bytes)
            .map_err(|_| ConfigError::InvalidUtf8)?
            .to_owned();
        if source.contains('\t') {
            return Err(ConfigError::TabCharacter);
        }
        let ast = parse_document(&source)?;
        validate_known_names(&ast, contributors)?;
        Ok(Self {
            source,
            ast,
            contributors: contributors.to_vec(),
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn contributors(&self) -> &[&'static dyn LanguageContributor] {
        &self.contributors
    }

    pub fn render(&self) -> String {
        self.source.clone()
    }

    pub fn upsert_scalar(
        &mut self,
        section: Option<&str>,
        key: &str,
        value: ScalarValue,
    ) -> Result<(), ConfigError> {
        if !is_name(key) {
            return Err(ConfigError::InvalidKey(key.to_owned()));
        }
        match self.scalar_edit(section, key) {
            ScalarEdit::Rejected(error) => Err(error),
            ScalarEdit::Replace(range) => self.replace_rendered_range(range, &value.render()),
            ScalarEdit::Insert => self.insert_scalar(section, key, &value),
        }
    }

    fn scalar_edit(&self, section: Option<&str>, key: &str) -> ScalarEdit {
        match self.ast.assignment(section, key) {
            Some(assignment) if !assignment.value.is_scalar() => {
                ScalarEdit::Rejected(ConfigError::NonScalarUpsert {
                    section: section.map(str::to_owned),
                    key: key.to_owned(),
                })
            }
            Some(assignment) => ScalarEdit::Replace(assignment.value_start..assignment.value_end),
            None => ScalarEdit::Insert,
        }
    }

    fn insert_scalar(
        &mut self,
        section: Option<&str>,
        key: &str,
        value: &ScalarValue,
    ) -> Result<(), ConfigError> {
        let line_ending = preferred_line_ending(&self.source);
        let insertion = format!("{key} = {}{line_ending}", value.render());
        let offset = self.insertion_offset(section)?;
        let prefix = insertion_prefix(&self.source, offset, line_ending);
        let mut rendered = self.source.clone();
        rendered.insert_str(offset, &format!("{prefix}{insertion}"));
        self.reparse(rendered)
    }

    fn insertion_offset(&self, section: Option<&str>) -> Result<usize, ConfigError> {
        match section {
            Some(name) => self
                .ast
                .section_insertion_offset(name)
                .ok_or_else(|| ConfigError::MissingSection(name.to_owned())),
            None => Ok(self.ast.root_insertion_offset),
        }
    }

    fn replace_rendered_range(
        &mut self,
        range: std::ops::Range<usize>,
        rendered_value: &str,
    ) -> Result<(), ConfigError> {
        let mut rendered = self.source.clone();
        rendered.replace_range(range, rendered_value);
        self.reparse(rendered)
    }

    fn reparse(&mut self, rendered: String) -> Result<(), ConfigError> {
        let contributors = self.contributors.clone();
        *self = Self::parse_with(&rendered, &contributors)?;
        Ok(())
    }

    pub fn section_names(&self) -> impl Iterator<Item = &str> {
        self.ast.section_order.iter().map(String::as_str)
    }

    pub fn optional_string(
        &self,
        section: Option<&str>,
        key: &str,
    ) -> Result<Option<String>, ConfigError> {
        crate::schema::optional_string(&self.ast, section, key)
    }

    pub fn optional_integer(
        &self,
        section: Option<&str>,
        key: &str,
    ) -> Result<Option<i64>, ConfigError> {
        crate::schema::optional_integer(&self.ast, section, key)
    }

    pub fn optional_boolean(
        &self,
        section: Option<&str>,
        key: &str,
    ) -> Result<Option<bool>, ConfigError> {
        crate::schema::optional_boolean(&self.ast, section, key)
    }

    pub fn optional_string_list(
        &self,
        section: Option<&str>,
        key: &str,
    ) -> Result<Option<Vec<String>>, ConfigError> {
        crate::schema::optional_string_list(&self.ast, section, key)
    }

    pub fn optional_object_list(
        &self,
        section: Option<&str>,
        key: &str,
    ) -> Result<Option<Vec<BTreeMap<String, ScalarValue>>>, ConfigError> {
        crate::schema::optional_object_list(&self.ast, section, key)
    }

    pub fn scalar_fields(&self, section: &str) -> BTreeMap<String, ScalarValue> {
        crate::schema::fields_for_section(&self.ast, section)
    }

    pub(crate) fn ast(&self) -> &DocumentAst {
        &self.ast
    }
}

impl fmt::Debug for ConfigDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConfigDocument")
            .field("source", &self.source)
            .finish()
    }
}

fn preferred_line_ending(source: &str) -> &'static str {
    source.find('\n').map_or("\n", |index| {
        if index > 0 && source.as_bytes()[index - 1] == b'\r' {
            "\r\n"
        } else {
            "\n"
        }
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigError {
    FileTooLarge {
        bytes: usize,
    },
    InvalidUtf8,
    TabCharacter,
    InvalidKey(String),
    Parse {
        line: usize,
        message: String,
    },
    DuplicateSection(String),
    DuplicateKey {
        section: Option<String>,
        key: String,
    },
    UnknownSection(String),
    UnknownField {
        section: Option<String>,
        key: String,
    },
    NonScalarUpsert {
        section: Option<String>,
        key: String,
    },
    MissingSection(String),
    InvalidSchema(String),
    InvalidValue {
        path: String,
        message: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FileTooLarge { .. }
            | Self::InvalidUtf8
            | Self::TabCharacter
            | Self::InvalidKey(_)
            | Self::Parse { .. }
            | Self::DuplicateSection(_) => write_syntax_error(self, formatter),
            Self::DuplicateKey { .. }
            | Self::UnknownSection(_)
            | Self::UnknownField { .. }
            | Self::NonScalarUpsert { .. }
            | Self::MissingSection(_)
            | Self::InvalidSchema(_)
            | Self::InvalidValue { .. } => write_schema_error(self, formatter),
        }
    }
}

fn write_syntax_error(error: &ConfigError, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    match error {
        ConfigError::FileTooLarge { bytes } => {
            write!(formatter, "configuration is too large: {bytes} bytes")
        }
        ConfigError::InvalidUtf8 => formatter.write_str("configuration must be UTF-8"),
        ConfigError::TabCharacter => formatter.write_str("configuration cannot contain tabs"),
        ConfigError::InvalidKey(key) => write!(formatter, "invalid configuration key: {key}"),
        ConfigError::Parse { line, message } => {
            write!(formatter, "configuration line {line}: {message}")
        }
        ConfigError::DuplicateSection(section) => {
            write!(formatter, "duplicate configuration section: {section}")
        }
        ConfigError::DuplicateKey { .. }
        | ConfigError::UnknownSection(_)
        | ConfigError::UnknownField { .. }
        | ConfigError::NonScalarUpsert { .. }
        | ConfigError::MissingSection(_)
        | ConfigError::InvalidSchema(_)
        | ConfigError::InvalidValue { .. } => Ok(()),
    }
}

fn write_schema_error(error: &ConfigError, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    match error {
        ConfigError::DuplicateKey { section, key } => {
            write_qualified(formatter, "duplicate configuration key", section, key)
        }
        ConfigError::UnknownSection(section) => {
            write!(formatter, "unknown configuration section: {section}")
        }
        ConfigError::UnknownField { section, key } => {
            write_qualified(formatter, "unknown configuration field", section, key)
        }
        ConfigError::NonScalarUpsert { section, key } => {
            write_qualified(formatter, "cannot replace non-scalar value", section, key)
        }
        other => write_value_error(other, formatter),
    }
}

fn write_value_error(error: &ConfigError, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    match error {
        ConfigError::MissingSection(section) => {
            write!(formatter, "missing configuration section: {section}")
        }
        ConfigError::InvalidSchema(message) => formatter.write_str(message),
        ConfigError::InvalidValue { path, message } => {
            write!(formatter, "invalid {path}: {message}")
        }
        ConfigError::FileTooLarge { .. }
        | ConfigError::InvalidUtf8
        | ConfigError::TabCharacter
        | ConfigError::InvalidKey(_)
        | ConfigError::Parse { .. }
        | ConfigError::DuplicateSection(_)
        | ConfigError::DuplicateKey { .. }
        | ConfigError::UnknownSection(_)
        | ConfigError::UnknownField { .. }
        | ConfigError::NonScalarUpsert { .. } => Ok(()),
    }
}

fn write_qualified(
    formatter: &mut fmt::Formatter<'_>,
    label: &str,
    section: &Option<String>,
    key: &str,
) -> fmt::Result {
    match section {
        Some(section) => write!(formatter, "{label}: {section}.{key}"),
        None => write!(formatter, "{label}: {key}"),
    }
}

fn insertion_prefix<'a>(source: &'a str, offset: usize, line_ending: &'a str) -> &'a str {
    if offset > 0 && !source[..offset].ends_with('\n') {
        line_ending
    } else {
        ""
    }
}

enum ScalarEdit {
    Replace(std::ops::Range<usize>),
    Insert,
    Rejected(ConfigError),
}

impl std::error::Error for ConfigError {}

#[derive(Clone, Copy)]
struct Line<'a> {
    number: usize,
    start: usize,
    content: &'a str,
}

fn parse_document(source: &str) -> Result<DocumentAst, ConfigError> {
    let lines = lines(source);
    let mut ast = DocumentAst {
        root_insertion_offset: source.len(),
        ..DocumentAst::default()
    };
    let mut state = ParseState {
        current_section: None,
        sections: BTreeSet::new(),
        keys: BTreeSet::new(),
        index: 0,
    };
    while state.index < lines.len() {
        parse_document_line(source, &lines, &mut ast, &mut state)?;
    }
    Ok(ast)
}

struct ParseState {
    current_section: Option<String>,
    sections: BTreeSet<String>,
    keys: BTreeSet<(Option<String>, String)>,
    index: usize,
}

fn parse_document_line(
    source: &str,
    lines: &[Line<'_>],
    ast: &mut DocumentAst,
    state: &mut ParseState,
) -> Result<(), ConfigError> {
    let line = lines[state.index];
    let visible = without_comment(line.content);
    let content = visible.trim();
    if content.is_empty() || line.content.starts_with(' ') {
        return blank_or_indented(line, content, state);
    }
    if content.starts_with('[') {
        return parse_document_section(source, line, content, ast, state);
    }
    parse_document_entry(line, visible, content, lines, ast, state)
}

fn blank_or_indented(
    line: Line<'_>,
    content: &str,
    state: &mut ParseState,
) -> Result<(), ConfigError> {
    if content.is_empty() {
        state.index += 1;
        return Ok(());
    }
    Err(parse_error(line.number, "unexpected indentation"))
}

fn parse_document_section(
    source: &str,
    line: Line<'_>,
    content: &str,
    ast: &mut DocumentAst,
    state: &mut ParseState,
) -> Result<(), ConfigError> {
    let section = parse_section(content, line.number)?;
    if !state.sections.insert(section.clone()) {
        return Err(ConfigError::DuplicateSection(section));
    }
    record_section_boundary(source, line, ast, &section);
    state.current_section = Some(section);
    state.index += 1;
    Ok(())
}

fn record_section_boundary(source: &str, line: Line<'_>, ast: &mut DocumentAst, section: &str) {
    if let Some(previous) = ast.sections.last_mut() {
        previous.insertion_offset = line.start;
    } else {
        ast.root_insertion_offset = line.start;
    }
    ast.section_order.push(section.to_owned());
    ast.sections.push(Section {
        name: section.to_owned(),
        insertion_offset: source.len(),
    });
}

fn parse_document_entry(
    line: Line<'_>,
    visible: &str,
    content: &str,
    lines: &[Line<'_>],
    ast: &mut DocumentAst,
    state: &mut ParseState,
) -> Result<(), ConfigError> {
    if let Some((key, value, value_start, value_end)) = parse_scalar_assignment(line, visible)? {
        push_assignment(
            ast,
            &mut state.keys,
            Assignment {
                section: state.current_section.clone(),
                key,
                value,
                value_start,
                value_end,
            },
        )?;
        state.index += 1;
        return Ok(());
    }
    parse_document_block(line, content, lines, ast, state)
}

fn parse_document_block(
    line: Line<'_>,
    content: &str,
    lines: &[Line<'_>],
    ast: &mut DocumentAst,
    state: &mut ParseState,
) -> Result<(), ConfigError> {
    let key = parse_block_key(content, line.number)?;
    let (value, next_index) = parse_block(lines, state.index + 1)?;
    push_assignment(
        ast,
        &mut state.keys,
        Assignment {
            section: state.current_section.clone(),
            key,
            value,
            value_start: line.start,
            value_end: line.start + line.content.len(),
        },
    )?;
    state.index = next_index;
    Ok(())
}

fn lines(source: &str) -> Vec<Line<'_>> {
    let mut result = Vec::new();
    let mut offset = 0;
    for (index, segment) in source.split_inclusive('\n').enumerate() {
        let content = segment
            .strip_suffix('\n')
            .unwrap_or(segment)
            .strip_suffix('\r')
            .unwrap_or(segment.strip_suffix('\n').unwrap_or(segment));
        result.push(Line {
            number: index + 1,
            start: offset,
            content,
        });
        offset += segment.len();
    }
    if source.is_empty() || !source.ends_with('\n') {
        if source.is_empty() {
            result.push(Line {
                number: 1,
                start: 0,
                content: "",
            });
        } else if result.is_empty() {
            result.push(Line {
                number: 1,
                start: 0,
                content: source,
            });
        }
    }
    result
}

fn without_comment(line: &str) -> &str {
    let mut state = QuoteScan::default();
    for (index, character) in line.char_indices() {
        if comment_boundary(&mut state, character) {
            return &line[..index];
        }
    }
    line
}

#[derive(Default)]
struct QuoteScan {
    escaped: bool,
    quoted: bool,
}

fn comment_boundary(state: &mut QuoteScan, character: char) -> bool {
    if consume_escape(state, character) {
        return false;
    }
    if character == '\"' {
        state.quoted = !state.quoted;
        return false;
    }
    !state.quoted && character == '#'
}

fn consume_escape(state: &mut QuoteScan, character: char) -> bool {
    if state.escaped {
        state.escaped = false;
        return true;
    }
    if state.quoted && character == '\\' {
        state.escaped = true;
        return true;
    }
    false
}

fn parse_section(value: &str, line: usize) -> Result<String, ConfigError> {
    if !value.ends_with(']') || value.len() < 3 {
        return Err(parse_error(line, "malformed section header"));
    }
    let path = &value[1..value.len() - 1];
    if path.split('.').any(|segment| !is_name(segment)) {
        return Err(parse_error(line, "invalid section path"));
    }
    Ok(path.to_owned())
}

fn parse_scalar_assignment(
    line: Line<'_>,
    visible: &str,
) -> Result<Option<(String, Value, usize, usize)>, ConfigError> {
    let Some(equals) = visible.find('=') else {
        return Ok(None);
    };
    let key = visible[..equals].trim();
    if !is_name(key) {
        return Err(parse_error(line.number, "invalid assignment key"));
    }
    let remainder = &visible[equals + 1..];
    let leading = remainder.len() - remainder.trim_start().len();
    let scalar_source = remainder.trim();
    if scalar_source == "[]" {
        return Ok(Some((
            key.to_owned(),
            Value::EmptyList,
            line.start + equals + 1 + leading,
            line.start + equals + 1 + leading + 2,
        )));
    }
    let scalar = parse_scalar(scalar_source, line.number)?;
    let value_start = line.start + equals + 1 + leading;
    Ok(Some((
        key.to_owned(),
        Value::Scalar(scalar),
        value_start,
        value_start + scalar_source.len(),
    )))
}

fn parse_block_key(value: &str, line: usize) -> Result<String, ConfigError> {
    let Some(key) = value.strip_suffix(':') else {
        return Err(parse_error(line, "expected assignment or block list"));
    };
    if !is_name(key) {
        return Err(parse_error(line, "invalid block-list key"));
    }
    Ok(key.to_owned())
}

fn parse_block(lines: &[Line<'_>], index: usize) -> Result<(Value, usize), ConfigError> {
    let Some(first) = lines.get(index).copied() else {
        return Err(ConfigError::Parse {
            line: 0,
            message: "block list cannot be empty".to_owned(),
        });
    };
    if !first.content.starts_with("  - ") {
        return Err(parse_error(
            first.number,
            "block item must use exactly two spaces",
        ));
    }
    let first_value = &first.content[4..];
    if parse_object_item(first_value, first.number)?.is_some() {
        return parse_object_block(lines, index);
    }
    parse_scalar_block(lines, index)
}

fn parse_object_block(lines: &[Line<'_>], mut index: usize) -> Result<(Value, usize), ConfigError> {
    let first = lines[index];
    let (key, value) = required_object_start(&first.content[4..], first.number)?;
    let mut objects = Vec::new();
    let mut object = BTreeMap::from([(key, value)]);
    index += 1;
    while index < lines.len() {
        if !extend_object_block(lines, &mut index, &mut objects, &mut object)? {
            break;
        }
    }
    objects.push(object);
    Ok((Value::ObjectList(objects), index))
}

fn required_object_start(value: &str, line: usize) -> Result<(String, ScalarValue), ConfigError> {
    let Some(item) = parse_object_item(value, line)? else {
        return Err(parse_error(
            line,
            "cannot mix scalar and object block items",
        ));
    };
    Ok(item)
}

fn extend_object_block(
    lines: &[Line<'_>],
    index: &mut usize,
    objects: &mut Vec<BTreeMap<String, ScalarValue>>,
    object: &mut BTreeMap<String, ScalarValue>,
) -> Result<bool, ConfigError> {
    let line = lines[*index];
    if line.content.starts_with("  - ") {
        start_next_object(line, objects, object)?;
        *index += 1;
        return Ok(true);
    }
    if line.content.starts_with("    ") {
        continue_object(line, object)?;
        *index += 1;
        return Ok(true);
    }
    Ok(false)
}

fn start_next_object(
    line: Line<'_>,
    objects: &mut Vec<BTreeMap<String, ScalarValue>>,
    object: &mut BTreeMap<String, ScalarValue>,
) -> Result<(), ConfigError> {
    let Some((key, value)) = parse_object_item(&line.content[4..], line.number)? else {
        return Err(parse_error(
            line.number,
            "cannot mix scalar and object block items",
        ));
    };
    objects.push(std::mem::take(object));
    object.insert(key, value);
    Ok(())
}

fn continue_object(
    line: Line<'_>,
    object: &mut BTreeMap<String, ScalarValue>,
) -> Result<(), ConfigError> {
    if line.content.as_bytes().get(4) == Some(&b' ') {
        return Err(parse_error(
            line.number,
            "object continuation must use exactly four spaces",
        ));
    }
    let (key, value) = parse_required_object_item(&line.content[4..], line.number)?;
    if object.insert(key, value).is_some() {
        return Err(parse_error(line.number, "duplicate object field"));
    }
    Ok(())
}

fn parse_scalar_block(lines: &[Line<'_>], mut index: usize) -> Result<(Value, usize), ConfigError> {
    let first = lines[index];
    let mut scalars = vec![parse_scalar(
        without_comment(&first.content[4..]).trim(),
        first.number,
    )?];
    index += 1;
    while index < lines.len() && lines[index].content.starts_with("  - ") {
        push_scalar_item(lines, &mut index, &mut scalars)?;
    }
    Ok((Value::ScalarList(scalars), index))
}

fn push_scalar_item(
    lines: &[Line<'_>],
    index: &mut usize,
    scalars: &mut Vec<ScalarValue>,
) -> Result<(), ConfigError> {
    let line = lines[*index];
    let item = &line.content[4..];
    if parse_object_item(item, line.number)?.is_some() {
        return Err(parse_error(
            line.number,
            "cannot mix scalar and object block items",
        ));
    }
    scalars.push(parse_scalar(without_comment(item).trim(), line.number)?);
    *index += 1;
    Ok(())
}

fn parse_object_item(
    value: &str,
    line: usize,
) -> Result<Option<(String, ScalarValue)>, ConfigError> {
    if unquoted_equals(without_comment(value)).is_none() {
        return Ok(None);
    }
    parse_required_object_item(value, line).map(Some)
}

fn parse_required_object_item(
    value: &str,
    line: usize,
) -> Result<(String, ScalarValue), ConfigError> {
    let visible = without_comment(value);
    let Some(equals) = unquoted_equals(visible) else {
        return Err(parse_error(line, "object item requires key = scalar"));
    };
    let key = visible[..equals].trim();
    if !is_name(key) {
        return Err(parse_error(line, "invalid object field"));
    }
    let scalar = parse_scalar(visible[equals + 1..].trim(), line)?;
    Ok((key.to_owned(), scalar))
}

fn parse_scalar(value: &str, line: usize) -> Result<ScalarValue, ConfigError> {
    if let Some(boolean) = parse_boolean(value) {
        return Ok(ScalarValue::Boolean(boolean));
    }
    if let Ok(integer) = value.parse::<i64>() {
        return Ok(ScalarValue::Integer(integer));
    }
    parse_scalar_string(value, line)
}

fn parse_boolean(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn parse_scalar_string(value: &str, line: usize) -> Result<ScalarValue, ConfigError> {
    if value.starts_with('\"') {
        return parse_quoted_string(value, line).map(ScalarValue::String);
    }
    Err(parse_error(
        line,
        "expected quoted string, boolean, integer, or []",
    ))
}

fn parse_quoted_string(value: &str, line: usize) -> Result<String, ConfigError> {
    let Some(body) = quoted_body(value) else {
        return Err(parse_error(line, "unterminated quoted string"));
    };
    decode_quoted_body(body, line)
}

fn quoted_body(value: &str) -> Option<&str> {
    value
        .strip_prefix('\"')
        .and_then(|rest| rest.strip_suffix('\"'))
}

fn decode_quoted_body(body: &str, line: usize) -> Result<String, ConfigError> {
    let mut output = String::new();
    let mut characters = body.chars();
    while let Some(character) = characters.next() {
        push_quoted_character(&mut output, &mut characters, character, line)?;
    }
    Ok(output)
}

fn push_quoted_character(
    output: &mut String,
    characters: &mut std::str::Chars<'_>,
    character: char,
    line: usize,
) -> Result<(), ConfigError> {
    if character == '\"' {
        return Err(parse_error(line, "unescaped quote in string"));
    }
    if character != '\\' {
        output.push(character);
        return Ok(());
    }
    push_escape(output, characters, line)
}

fn push_escape(
    output: &mut String,
    characters: &mut std::str::Chars<'_>,
    line: usize,
) -> Result<(), ConfigError> {
    let Some(escape) = characters.next() else {
        return Err(parse_error(line, "incomplete string escape"));
    };
    output.push(decoded_escape(escape).ok_or_else(|| parse_error(line, "invalid string escape"))?);
    Ok(())
}

fn decoded_escape(escape: char) -> Option<char> {
    match escape {
        '\\' => Some('\\'),
        '\"' => Some('\"'),
        'n' => Some('\n'),
        'r' => Some('\r'),
        't' => Some('\t'),
        _ => None,
    }
}

fn unquoted_equals(value: &str) -> Option<usize> {
    let mut state = QuoteScan::default();
    for (index, character) in value.char_indices() {
        if equals_boundary(&mut state, character) {
            return Some(index);
        }
    }
    None
}

fn equals_boundary(state: &mut QuoteScan, character: char) -> bool {
    if consume_escape(state, character) {
        return false;
    }
    if character == '\"' {
        state.quoted = !state.quoted;
        return false;
    }
    !state.quoted && character == '='
}

fn push_assignment(
    ast: &mut DocumentAst,
    keys: &mut BTreeSet<(Option<String>, String)>,
    assignment: Assignment,
) -> Result<(), ConfigError> {
    if !keys.insert((assignment.section.clone(), assignment.key.clone())) {
        return Err(ConfigError::DuplicateKey {
            section: assignment.section,
            key: assignment.key,
        });
    }
    ast.assignments.push(assignment);
    Ok(())
}

fn validate_known_names(
    ast: &DocumentAst,
    contributors: &[&'static dyn LanguageContributor],
) -> Result<(), ConfigError> {
    validate_sections(ast, contributors)?;
    validate_fields(ast, contributors)
}

fn validate_sections(
    ast: &DocumentAst,
    contributors: &[&'static dyn LanguageContributor],
) -> Result<(), ConfigError> {
    for section in &ast.section_order {
        if !known_section(section, contributors) {
            return Err(ConfigError::UnknownSection(section.clone()));
        }
    }
    Ok(())
}

fn known_section(section: &str, contributors: &[&'static dyn LanguageContributor]) -> bool {
    core_section(section)
        || contributors
            .iter()
            .any(|contributor| contributor.owns_section(section))
}

fn validate_fields(
    ast: &DocumentAst,
    contributors: &[&'static dyn LanguageContributor],
) -> Result<(), ConfigError> {
    for assignment in &ast.assignments {
        if !known_assignment(assignment, contributors) {
            return Err(ConfigError::UnknownField {
                section: assignment.section.clone(),
                key: assignment.key.clone(),
            });
        }
    }
    Ok(())
}

fn known_assignment(
    assignment: &Assignment,
    contributors: &[&'static dyn LanguageContributor],
) -> bool {
    match assignment.section.as_deref() {
        None => root_field(&assignment.key),
        Some("project") => project_field(&assignment.key),
        Some("license") => license_field(&assignment.key),
        Some("generation") => assignment.key == "strict-license-files",
        Some(name) => contributor_field(contributors, name, &assignment.key),
    }
}

fn root_field(key: &str) -> bool {
    matches!(key, "schema" | "materials-directory" | "languages")
}

fn project_field(key: &str) -> bool {
    matches!(
        key,
        "name"
            | "canonical-repository"
            | "canonical-branch"
            | "right-holders"
            | "contact"
            | "adoption-date"
    )
}

fn license_field(key: &str) -> bool {
    matches!(
        key,
        "version" | "enabled" | "covered-scope" | "special-authorization-channel"
    )
}

fn contributor_field(
    contributors: &[&'static dyn LanguageContributor],
    name: &str,
    key: &str,
) -> bool {
    contributors
        .iter()
        .any(|contributor| contributor.owns_section(name) && contributor.known_field(name, key))
}

fn core_section(section: &str) -> bool {
    matches!(section, "project" | "license" | "generation")
}

fn is_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn parse_error(line: usize, message: &str) -> ConfigError {
    ConfigError::Parse {
        line,
        message: message.to_owned(),
    }
}
