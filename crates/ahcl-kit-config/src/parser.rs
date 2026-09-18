use crate::ast::{Assignment, DocumentAst, ScalarValue, Section, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const MAX_CONFIG_BYTES: usize = 1_048_576;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigDocument {
    source: String,
    ast: DocumentAst,
}

impl ConfigDocument {
    pub fn parse(source: &str) -> Result<Self, ConfigError> {
        Self::parse_bytes(source.as_bytes())
    }

    pub fn parse_bytes(bytes: &[u8]) -> Result<Self, ConfigError> {
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
        validate_known_names(&ast)?;
        Ok(Self { source, ast })
    }

    pub fn source(&self) -> &str {
        &self.source
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
        if let Some(assignment) = self.ast.assignment(section, key) {
            if !assignment.value.is_scalar() {
                return Err(ConfigError::NonScalarUpsert {
                    section: section.map(str::to_owned),
                    key: key.to_owned(),
                });
            }
            let mut rendered = self.source.clone();
            rendered.replace_range(
                assignment.value_start..assignment.value_end,
                &value.render(),
            );
            *self = Self::parse(&rendered)?;
            return Ok(());
        }

        let insertion = format!("{key} = {}\n", value.render());
        let offset = match section {
            Some(name) => self
                .ast
                .section_insertion_offset(name)
                .ok_or_else(|| ConfigError::MissingSection(name.to_owned()))?,
            None => self.ast.root_insertion_offset,
        };
        let mut rendered = self.source.clone();
        let prefix = if offset > 0 && !self.source[..offset].ends_with('\n') {
            "\n"
        } else {
            ""
        };
        rendered.insert_str(offset, &format!("{prefix}{insertion}"));
        *self = Self::parse(&rendered)?;
        Ok(())
    }

    pub(crate) fn ast(&self) -> &DocumentAst {
        &self.ast
    }
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
            Self::FileTooLarge { bytes } => {
                write!(formatter, "configuration is too large: {bytes} bytes")
            }
            Self::InvalidUtf8 => formatter.write_str("configuration must be UTF-8"),
            Self::TabCharacter => formatter.write_str("configuration cannot contain tabs"),
            Self::InvalidKey(key) => write!(formatter, "invalid configuration key: {key}"),
            Self::Parse { line, message } => {
                write!(formatter, "configuration line {line}: {message}")
            }
            Self::DuplicateSection(section) => {
                write!(formatter, "duplicate configuration section: {section}")
            }
            Self::DuplicateKey { section, key } => match section {
                Some(section) => write!(formatter, "duplicate configuration key: {section}.{key}"),
                None => write!(formatter, "duplicate configuration key: {key}"),
            },
            Self::UnknownSection(section) => {
                write!(formatter, "unknown configuration section: {section}")
            }
            Self::UnknownField { section, key } => match section {
                Some(section) => write!(formatter, "unknown configuration field: {section}.{key}"),
                None => write!(formatter, "unknown configuration field: {key}"),
            },
            Self::NonScalarUpsert { section, key } => match section {
                Some(section) => write!(
                    formatter,
                    "cannot replace non-scalar value: {section}.{key}"
                ),
                None => write!(formatter, "cannot replace non-scalar value: {key}"),
            },
            Self::MissingSection(section) => {
                write!(formatter, "missing configuration section: {section}")
            }
            Self::InvalidSchema(message) => formatter.write_str(message),
            Self::InvalidValue { path, message } => write!(formatter, "invalid {path}: {message}"),
        }
    }
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
    let mut current_section = None;
    let mut sections = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        let visible = without_comment(line.content);
        let content = visible.trim();
        if content.is_empty() {
            index += 1;
            continue;
        }
        if line.content.starts_with(' ') {
            return Err(parse_error(line.number, "unexpected indentation"));
        }
        if content.starts_with('[') {
            let section = parse_section(content, line.number)?;
            if !sections.insert(section.clone()) {
                return Err(ConfigError::DuplicateSection(section));
            }
            if let Some(previous) = ast.sections.last_mut() {
                previous.insertion_offset = line.start;
            } else {
                ast.root_insertion_offset = line.start;
            }
            ast.section_order.push(section.clone());
            ast.sections.push(Section {
                name: section.clone(),
                insertion_offset: source.len(),
            });
            current_section = Some(section);
            index += 1;
            continue;
        }

        if let Some((key, value, value_start, value_end)) = parse_scalar_assignment(line, visible)?
        {
            push_assignment(
                &mut ast,
                &mut keys,
                Assignment {
                    section: current_section.clone(),
                    key,
                    value,
                    value_start,
                    value_end,
                },
            )?;
            index += 1;
            continue;
        }

        let key = parse_block_key(content, line.number)?;
        let (value, next_index) = parse_block(&lines, index + 1)?;
        push_assignment(
            &mut ast,
            &mut keys,
            Assignment {
                section: current_section.clone(),
                key,
                value,
                value_start: line.start,
                value_end: line.start + line.content.len(),
            },
        )?;
        index = next_index;
    }

    Ok(ast)
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
    let mut escaped = false;
    let mut quoted = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
        } else if quoted && character == '\\' {
            escaped = true;
        } else if character == '\"' {
            quoted = !quoted;
        } else if !quoted && character == '#' {
            return &line[..index];
        }
    }
    line
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

fn parse_block(lines: &[Line<'_>], mut index: usize) -> Result<(Value, usize), ConfigError> {
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
    if let Some((key, value)) = parse_object_item(first_value, first.number)? {
        let mut objects = Vec::new();
        let mut object = BTreeMap::new();
        object.insert(key, value);
        index += 1;
        while index < lines.len() {
            let line = lines[index];
            if line.content.starts_with("  - ") {
                let next = &line.content[4..];
                let Some((key, value)) = parse_object_item(next, line.number)? else {
                    return Err(parse_error(
                        line.number,
                        "cannot mix scalar and object block items",
                    ));
                };
                objects.push(object);
                object = BTreeMap::new();
                object.insert(key, value);
                index += 1;
            } else if line.content.starts_with("    ") {
                if line.content.as_bytes().get(4) == Some(&b' ') {
                    return Err(parse_error(
                        line.number,
                        "object continuation must use exactly four spaces",
                    ));
                }
                let (key, value) = parse_required_object_item(&line.content[4..], line.number)?;
                if object.insert(key.clone(), value).is_some() {
                    return Err(parse_error(line.number, "duplicate object field"));
                }
                index += 1;
            } else {
                break;
            }
        }
        objects.push(object);
        return Ok((Value::ObjectList(objects), index));
    }

    let mut scalars = vec![parse_scalar(
        without_comment(first_value).trim(),
        first.number,
    )?];
    index += 1;
    while index < lines.len() && lines[index].content.starts_with("  - ") {
        let line = lines[index];
        let item = &line.content[4..];
        if parse_object_item(item, line.number)?.is_some() {
            return Err(parse_error(
                line.number,
                "cannot mix scalar and object block items",
            ));
        }
        scalars.push(parse_scalar(without_comment(item).trim(), line.number)?);
        index += 1;
    }
    Ok((Value::ScalarList(scalars), index))
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
    if value == "true" {
        return Ok(ScalarValue::Boolean(true));
    }
    if value == "false" {
        return Ok(ScalarValue::Boolean(false));
    }
    if let Ok(integer) = value.parse::<i64>() {
        return Ok(ScalarValue::Integer(integer));
    }
    if value.starts_with('\"') {
        return parse_quoted_string(value, line).map(ScalarValue::String);
    }
    Err(parse_error(
        line,
        "expected quoted string, boolean, integer, or []",
    ))
}

fn parse_quoted_string(value: &str, line: usize) -> Result<String, ConfigError> {
    let Some(body) = value
        .strip_prefix('\"')
        .and_then(|rest| rest.strip_suffix('\"'))
    else {
        return Err(parse_error(line, "unterminated quoted string"));
    };
    let mut output = String::new();
    let mut characters = body.chars();
    while let Some(character) = characters.next() {
        if character == '\"' {
            return Err(parse_error(line, "unescaped quote in string"));
        }
        if character != '\\' {
            output.push(character);
            continue;
        }
        let Some(escape) = characters.next() else {
            return Err(parse_error(line, "incomplete string escape"));
        };
        match escape {
            '\\' => output.push('\\'),
            '\"' => output.push('\"'),
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            _ => return Err(parse_error(line, "invalid string escape")),
        }
    }
    Ok(output)
}

fn unquoted_equals(value: &str) -> Option<usize> {
    let mut escaped = false;
    let mut quoted = false;
    for (index, character) in value.char_indices() {
        if escaped {
            escaped = false;
        } else if quoted && character == '\\' {
            escaped = true;
        } else if character == '\"' {
            quoted = !quoted;
        } else if !quoted && character == '=' {
            return Some(index);
        }
    }
    None
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

fn validate_known_names(ast: &DocumentAst) -> Result<(), ConfigError> {
    for section in &ast.section_order {
        if !matches!(
            section.as_str(),
            "project" | "license" | "generation" | "rust.cargo"
        ) {
            return Err(ConfigError::UnknownSection(section.clone()));
        }
    }
    for assignment in &ast.assignments {
        let valid = match assignment.section.as_deref() {
            None => matches!(
                assignment.key.as_str(),
                "schema" | "materials-directory" | "languages"
            ),
            Some("project") => matches!(
                assignment.key.as_str(),
                "name"
                    | "canonical-repository"
                    | "canonical-branch"
                    | "right-holders"
                    | "contact"
                    | "adoption-date"
            ),
            Some("license") => matches!(
                assignment.key.as_str(),
                "version" | "special-authorization-channel"
            ),
            Some("generation") => assignment.key == "strict-license-files",
            Some("rust.cargo") => matches!(
                assignment.key.as_str(),
                "manifests" | "packages" | "rules" | "lock-mode"
            ),
            Some(_) => false,
        };
        if !valid {
            return Err(ConfigError::UnknownField {
                section: assignment.section.clone(),
                key: assignment.key.clone(),
            });
        }
    }
    Ok(())
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
