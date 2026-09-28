// crates/ahcl-kit-javascript/src/pnpm.rs - pnpm-lock.yaml resolution.
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

use crate::adapter::{json_string, manifest_value};
use crate::error::JavascriptError;
use crate::model::{ParsedEdge, ParsedGraph, ParsedPackage};
use ahcl_kit_core::{DependencyKind, LockfileEvidence, RepoPath};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Yaml {
    Scalar(String),
    Map(BTreeMap<String, Yaml>),
}

pub(crate) fn parse_pnpm(
    manifest_text: &str,
    lock_text: &str,
    manifest_path: &Path,
    lockfile: LockfileEvidence,
) -> Result<ParsedGraph, JavascriptError> {
    let manifest = manifest_value(manifest_text).map_err(parse_error)?;
    let root = parse_yaml(lock_text).map_err(parse_error)?;
    let loaded = load_pnpm_lock(&root)?;
    finish_pnpm_graph(&loaded, &manifest, manifest_path, lockfile)
}

struct PnpmLock {
    version_nine: bool,
    packages: BTreeMap<String, Yaml>,
    snapshots: BTreeMap<String, Yaml>,
    importers: BTreeMap<String, Yaml>,
}

struct PnpmGraph {
    packages: Vec<ParsedPackage>,
    edges: Vec<ParsedEdge>,
    importer_ids: BTreeMap<String, String>,
}

fn load_pnpm_lock(root: &BTreeMap<String, Yaml>) -> Result<PnpmLock, JavascriptError> {
    let lockfile_version = pnpm_lock_version(root);
    require_pnpm_version(&lockfile_version)?;
    Ok(PnpmLock {
        version_nine: lockfile_version.starts_with('9'),
        packages: map(root, "packages").cloned().unwrap_or_default(),
        snapshots: map(root, "snapshots").cloned().unwrap_or_default(),
        importers: pnpm_importers(root),
    })
}

fn pnpm_lock_version(root: &BTreeMap<String, Yaml>) -> String {
    scalar(root, "lockfileVersion")
        .unwrap_or_default()
        .trim_matches(['\'', '"'])
        .to_owned()
}

fn require_pnpm_version(lockfile_version: &str) -> Result<(), JavascriptError> {
    if lockfile_version.starts_with('6') || lockfile_version.starts_with('9') {
        return Ok(());
    }
    Err(parse_error(format!(
        "unsupported pnpm lockfileVersion {lockfile_version}; require 6 or 9"
    )))
}

fn pnpm_importers(root: &BTreeMap<String, Yaml>) -> BTreeMap<String, Yaml> {
    if let Some(importers) = map(root, "importers") {
        return importers.clone();
    }
    let mut synthetic = BTreeMap::new();
    synthetic.insert(".".to_owned(), Yaml::Map(synthetic_importer(root)));
    synthetic
}

fn synthetic_importer(root: &BTreeMap<String, Yaml>) -> BTreeMap<String, Yaml> {
    let mut importer = BTreeMap::new();
    for key in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        if let Some(value) = root.get(key) {
            importer.insert(key.to_owned(), value.clone());
        }
    }
    importer
}

fn importer_packages(
    importers: &BTreeMap<String, Yaml>,
    manifest: &serde_json::Value,
    manifest_path: &Path,
    lockfile: &LockfileEvidence,
) -> Result<PnpmGraph, JavascriptError> {
    let mut graph = PnpmGraph {
        packages: Vec::new(),
        edges: Vec::new(),
        importer_ids: BTreeMap::new(),
    };
    for (importer_path, importer) in importers {
        push_importer_package(
            &mut graph,
            importer_path,
            importer,
            &PnpmEmit {
                manifest,
                manifest_path,
                lockfile,
            },
        )?;
    }
    Ok(graph)
}

struct PnpmEmit<'a> {
    manifest: &'a serde_json::Value,
    manifest_path: &'a Path,
    lockfile: &'a LockfileEvidence,
}

fn finish_pnpm_graph(
    loaded: &PnpmLock,
    manifest: &serde_json::Value,
    manifest_path: &Path,
    lockfile: LockfileEvidence,
) -> Result<ParsedGraph, JavascriptError> {
    let mut graph = importer_packages(&loaded.importers, manifest, manifest_path, &lockfile)?;
    push_pnpm_packages(loaded, manifest_path, &lockfile, &mut graph)?;
    push_recorded_edges(loaded, &mut graph)?;
    Ok(ParsedGraph {
        packages: graph.packages,
        edges: graph.edges,
    })
}

fn push_recorded_edges(loaded: &PnpmLock, graph: &mut PnpmGraph) -> Result<(), JavascriptError> {
    push_importer_package_edges(loaded, &graph.importer_ids, &mut graph.edges)?;
    if loaded.version_nine {
        push_snapshot_edges(loaded, &graph.importer_ids, &mut graph.edges)?;
    }
    Ok(())
}

fn push_importer_package(
    graph: &mut PnpmGraph,
    importer_path: &str,
    importer: &Yaml,
    emit: &PnpmEmit<'_>,
) -> Result<(), JavascriptError> {
    let Yaml::Map(_) = importer else {
        return Err(parse_error(format!(
            "pnpm importer {importer_path} is invalid"
        )));
    };
    let (name, version, license) = importer_identity(importer_path, emit.manifest);
    let id = format!("pnpm:importer:{importer_path}:{name}@{version}");
    graph
        .importer_ids
        .insert(importer_path.to_owned(), id.clone());
    graph.packages.push(ParsedPackage {
        id,
        name,
        version,
        source: Some("workspace".to_owned()),
        checksum: None,
        declared_license: license,
        workspace_root: true,
        manifest_path: emit.manifest_path.to_path_buf(),
        lockfiles: vec![emit.lockfile.clone()],
    });
    Ok(())
}

fn importer_identity(
    importer_path: &str,
    manifest: &serde_json::Value,
) -> (String, String, Option<String>) {
    if importer_path == "." {
        return (
            json_string(manifest, "name").unwrap_or_else(|| "workspace".to_owned()),
            json_string(manifest, "version").unwrap_or_else(|| "0.0.0".to_owned()),
            json_string(manifest, "license"),
        );
    }
    (importer_path.to_owned(), "0.0.0".to_owned(), None)
}

fn push_pnpm_packages(
    loaded: &PnpmLock,
    manifest_path: &Path,
    lockfile: &LockfileEvidence,
    graph: &mut PnpmGraph,
) -> Result<(), JavascriptError> {
    for (key, value) in &loaded.packages {
        push_pnpm_package(loaded, key, value, manifest_path, lockfile, graph)?;
    }
    Ok(())
}

fn push_pnpm_package(
    loaded: &PnpmLock,
    key: &str,
    value: &Yaml,
    manifest_path: &Path,
    lockfile: &LockfileEvidence,
    graph: &mut PnpmGraph,
) -> Result<(), JavascriptError> {
    let Yaml::Map(fields) = value else {
        return Ok(());
    };
    let (name, version) = package_identity(key)
        .ok_or_else(|| parse_error(format!("pnpm package key is invalid: {key}")))?;
    let id = format!("pnpm:{}", strip_peer(key));
    if !loaded.version_nine {
        add_dependency_edges(
            &id,
            fields,
            &loaded.packages,
            &graph.importer_ids,
            &mut graph.edges,
        )?;
    }
    graph.packages.push(parsed_pnpm_package(
        fields,
        manifest_path,
        lockfile,
        id,
        name,
        version,
    ));
    Ok(())
}

fn parsed_pnpm_package(
    fields: &BTreeMap<String, Yaml>,
    manifest_path: &Path,
    lockfile: &LockfileEvidence,
    id: String,
    name: String,
    version: String,
) -> ParsedPackage {
    let resolution = map_from(fields, "resolution");
    ParsedPackage {
        id,
        name,
        version,
        source: pnpm_source(resolution),
        checksum: resolution
            .and_then(|resolution| scalar(resolution, "integrity").map(str::to_owned)),
        declared_license: None,
        workspace_root: false,
        manifest_path: manifest_path.to_path_buf(),
        lockfiles: vec![lockfile.clone()],
    }
}

fn pnpm_source(resolution: Option<&BTreeMap<String, Yaml>>) -> Option<String> {
    resolution
        .and_then(|resolution| scalar(resolution, "tarball").map(str::to_owned))
        .or_else(|| Some("registry".to_owned()))
}

fn push_importer_package_edges(
    loaded: &PnpmLock,
    importer_ids: &BTreeMap<String, String>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    for (importer_path, importer) in &loaded.importers {
        push_one_importer_edges(
            importer_path,
            importer,
            &loaded.packages,
            importer_ids,
            edges,
        )?;
    }
    Ok(())
}

fn push_one_importer_edges(
    importer_path: &str,
    importer: &Yaml,
    packages: &BTreeMap<String, Yaml>,
    importer_ids: &BTreeMap<String, String>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    let Yaml::Map(fields) = importer else {
        return Ok(());
    };
    let Some(id) = importer_ids.get(importer_path) else {
        return Ok(());
    };
    add_importer_edges(id, fields, packages, importer_ids, edges)
}

fn push_snapshot_edges(
    loaded: &PnpmLock,
    importer_ids: &BTreeMap<String, String>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    for (key, value) in &loaded.snapshots {
        push_one_snapshot(key, value, &loaded.packages, importer_ids, edges)?;
    }
    Ok(())
}

fn push_one_snapshot(
    key: &str,
    value: &Yaml,
    packages: &BTreeMap<String, Yaml>,
    importer_ids: &BTreeMap<String, String>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    let Yaml::Map(fields) = value else {
        return Ok(());
    };
    let id = format!("pnpm:{}", strip_peer(key));
    add_dependency_edges(&id, fields, packages, importer_ids, edges)
}

fn add_importer_edges(
    from: &str,
    fields: &BTreeMap<String, Yaml>,
    packages: &BTreeMap<String, Yaml>,
    importers: &BTreeMap<String, String>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    for (key, kind, target) in [
        ("dependencies", DependencyKind::Normal, None),
        (
            "optionalDependencies",
            DependencyKind::Normal,
            Some("optional"),
        ),
        ("peerDependencies", DependencyKind::Normal, Some("peer")),
        ("devDependencies", DependencyKind::Development, None),
    ] {
        let edge = ImporterEdge {
            from,
            packages,
            importers,
            key,
            kind,
            target,
        };
        add_importer_group(&edge, fields, edges)?;
    }
    Ok(())
}

struct ImporterEdge<'a> {
    from: &'a str,
    packages: &'a BTreeMap<String, Yaml>,
    importers: &'a BTreeMap<String, String>,
    key: &'a str,
    kind: DependencyKind,
    target: Option<&'static str>,
}

fn add_importer_group(
    edge: &ImporterEdge<'_>,
    fields: &BTreeMap<String, Yaml>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    let Some(Yaml::Map(dependencies)) = fields.get(edge.key) else {
        return Ok(());
    };
    for (name, value) in dependencies {
        push_importer_dependency(edge, name, value, edges)?;
    }
    Ok(())
}

fn push_importer_dependency(
    edge: &ImporterEdge<'_>,
    name: &str,
    value: &Yaml,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    let version = dependency_version(value)
        .ok_or_else(|| parse_error(format!("pnpm dependency {name} has no version")))?;
    let Some(to) = resolve_pnpm_target(edge.packages, edge.importers, name, version) else {
        return missing_importer_target(edge.key, name, version);
    };
    edges.push(ParsedEdge {
        from: edge.from.to_owned(),
        to,
        kind: edge.kind,
        targets: edge
            .target
            .map(|value| vec![value.to_owned()])
            .unwrap_or_default(),
    });
    Ok(())
}

fn missing_importer_target(key: &str, name: &str, version: &str) -> Result<(), JavascriptError> {
    if version.starts_with("link:") || optional_importer_key(key) {
        return Ok(());
    }
    Err(parse_error(format!(
        "pnpm dependency {name}@{version} was not found"
    )))
}

fn optional_importer_key(key: &str) -> bool {
    matches!(key, "optionalDependencies" | "peerDependencies")
}

fn add_dependency_edges(
    from: &str,
    fields: &BTreeMap<String, Yaml>,
    packages: &BTreeMap<String, Yaml>,
    importers: &BTreeMap<String, String>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    for key in ["dependencies", "optionalDependencies"] {
        add_dependency_group(
            &DependencyEdge {
                from,
                packages,
                importers,
                key,
            },
            fields,
            edges,
        )?;
    }
    Ok(())
}

struct DependencyEdge<'a> {
    from: &'a str,
    packages: &'a BTreeMap<String, Yaml>,
    importers: &'a BTreeMap<String, String>,
    key: &'a str,
}

fn add_dependency_group(
    edge: &DependencyEdge<'_>,
    fields: &BTreeMap<String, Yaml>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    let Some(Yaml::Map(dependencies)) = fields.get(edge.key) else {
        return Ok(());
    };
    for (name, value) in dependencies {
        push_dependency(edge, name, value, edges)?;
    }
    Ok(())
}

fn push_dependency(
    edge: &DependencyEdge<'_>,
    name: &str,
    value: &Yaml,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    let Some(version) = dependency_version(value) else {
        return missing_dependency_version(edge.key, name);
    };
    let Some(to) = resolve_pnpm_target(edge.packages, edge.importers, name, version) else {
        return missing_dependency_target(edge.key, name, version);
    };
    edges.push(ParsedEdge {
        from: edge.from.to_owned(),
        to,
        kind: DependencyKind::Normal,
        targets: dependency_targets(edge.key),
    });
    Ok(())
}

fn missing_dependency_version(key: &str, name: &str) -> Result<(), JavascriptError> {
    if key == "optionalDependencies" {
        return Ok(());
    }
    Err(parse_error(format!(
        "pnpm dependency {name} has no version"
    )))
}

fn missing_dependency_target(key: &str, name: &str, version: &str) -> Result<(), JavascriptError> {
    if key == "optionalDependencies" || version.starts_with("link:") {
        return Ok(());
    }
    Err(parse_error(format!(
        "pnpm dependency {name}@{version} was not found"
    )))
}

fn dependency_targets(key: &str) -> Vec<String> {
    if key == "optionalDependencies" {
        vec!["optional".to_owned()]
    } else {
        Vec::new()
    }
}

fn resolve_pnpm_target(
    packages: &BTreeMap<String, Yaml>,
    importers: &BTreeMap<String, String>,
    name: &str,
    version: &str,
) -> Option<String> {
    if let Some(path) = version.strip_prefix("link:") {
        let path = path.trim_matches('/');
        let path = if path.is_empty() || path == "." {
            "."
        } else {
            path
        };
        return importers.get(path).cloned();
    }
    find_package(packages, name, strip_peer(version)).map(|key| format!("pnpm:{key}"))
}

fn strip_peer(value: &str) -> &str {
    value.split_once('(').map(|(base, _)| base).unwrap_or(value)
}

fn dependency_version(value: &Yaml) -> Option<&str> {
    match value {
        Yaml::Scalar(version) => Some(version.as_str()),
        Yaml::Map(fields) => scalar(fields, "version"),
    }
}

fn find_package(packages: &BTreeMap<String, Yaml>, name: &str, version: &str) -> Option<String> {
    [
        format!("{name}@{version}"),
        format!("/{name}@{version}"),
        format!("/{name}/{version}"),
    ]
    .into_iter()
    .find(|candidate| packages.contains_key(candidate))
}

fn package_identity(key: &str) -> Option<(String, String)> {
    let key = key.trim_start_matches('/');
    let (name, version) = if let Some(rest) = key.strip_prefix('@') {
        let (scoped, version) = rest.rsplit_once('@').or_else(|| rest.rsplit_once('/'))?;
        (format!("@{scoped}"), version)
    } else {
        let (name, version) = key.rsplit_once('@').or_else(|| key.rsplit_once('/'))?;
        (name.to_owned(), version)
    };
    let version = version.split('(').next().unwrap_or(version);
    if name.is_empty() || version.is_empty() {
        return None;
    }
    Some((name, version.to_owned()))
}

fn scalar<'a>(map: &'a BTreeMap<String, Yaml>, key: &str) -> Option<&'a str> {
    match map.get(key) {
        Some(Yaml::Scalar(value)) => Some(value.as_str()),
        _ => None,
    }
}

fn map<'a>(map: &'a BTreeMap<String, Yaml>, key: &str) -> Option<&'a BTreeMap<String, Yaml>> {
    match map.get(key) {
        Some(Yaml::Map(value)) => Some(value),
        _ => None,
    }
}

fn map_from<'a>(map: &'a BTreeMap<String, Yaml>, key: &str) -> Option<&'a BTreeMap<String, Yaml>> {
    self::map(map, key)
}

fn parse_yaml(text: &str) -> Result<BTreeMap<String, Yaml>, String> {
    let lines = text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| {
            let trimmed = line.trim_start_matches(' ');
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let borrowed = lines.iter().map(String::as_str).collect::<Vec<_>>();
    let mut index = 0;
    parse_block(&borrowed, &mut index, 0)
}

fn parse_block(
    lines: &[&str],
    index: &mut usize,
    indent: usize,
) -> Result<BTreeMap<String, Yaml>, String> {
    let mut map = BTreeMap::new();
    while *index < lines.len() {
        if !parse_block_line(lines, index, indent, &mut map)? {
            break;
        }
    }
    Ok(map)
}

fn parse_block_line(
    lines: &[&str],
    index: &mut usize,
    indent: usize,
    map: &mut BTreeMap<String, Yaml>,
) -> Result<bool, String> {
    let line = lines[*index];
    reject_pnpm_tabs(line)?;
    if !indent_continues(indentation(line), indent, line)? {
        return Ok(false);
    }
    insert_block_entry(lines, index, indent, map)?;
    Ok(true)
}

fn reject_pnpm_tabs(line: &str) -> Result<(), String> {
    if line.contains('\t') {
        return Err("pnpm lockfile cannot contain tabs".to_owned());
    }
    Ok(())
}

fn indentation(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn indent_continues(current: usize, indent: usize, line: &str) -> Result<bool, String> {
    if current < indent {
        return Ok(false);
    }
    if current != indent {
        return Err(format!("unexpected indentation in pnpm lockfile: {line}"));
    }
    Ok(true)
}

fn insert_block_entry(
    lines: &[&str],
    index: &mut usize,
    indent: usize,
    map: &mut BTreeMap<String, Yaml>,
) -> Result<(), String> {
    let trimmed = lines[*index].trim_start_matches(' ');
    reject_pnpm_list(trimmed)?;
    let (key, value) = split_entry(trimmed)?;
    *index += 1;
    map.insert(key, parse_block_value(lines, index, indent, value)?);
    Ok(())
}

fn reject_pnpm_list(trimmed: &str) -> Result<(), String> {
    if trimmed.starts_with("- ") {
        return Err("pnpm lockfile lists are not supported".to_owned());
    }
    Ok(())
}

fn parse_block_value(
    lines: &[&str],
    index: &mut usize,
    indent: usize,
    value: Option<&str>,
) -> Result<Yaml, String> {
    if let Some(value) = value {
        return parse_scalar_or_inline(value);
    }
    nested_block(lines, index, indent)
}

fn nested_block(lines: &[&str], index: &mut usize, indent: usize) -> Result<Yaml, String> {
    if *index >= lines.len() {
        return Ok(Yaml::Map(BTreeMap::new()));
    }
    let next_indent = indentation(lines[*index]);
    if next_indent > indent {
        return Ok(Yaml::Map(parse_block(lines, index, next_indent)?));
    }
    Ok(Yaml::Map(BTreeMap::new()))
}

fn split_entry(line: &str) -> Result<(String, Option<&str>), String> {
    let mut quoted = None;
    for (index, character) in line.char_indices() {
        if let Some(split) = split_entry_at(line, index, character, &mut quoted)? {
            return Ok(split);
        }
    }
    Err(format!("pnpm lockfile entry has no key: {line}"))
}

fn split_entry_at<'a>(
    line: &'a str,
    index: usize,
    character: char,
    quoted: &mut Option<char>,
) -> Result<Option<(String, Option<&'a str>)>, String> {
    match (*quoted, character) {
        (None, '\'' | '"') => {
            *quoted = Some(character);
            Ok(None)
        }
        (Some(quote), character) if character == quote => {
            *quoted = None;
            Ok(None)
        }
        (None, ':') => Ok(Some(split_key_value(line, index)?)),
        _ => Ok(None),
    }
}

fn split_key_value(line: &str, index: usize) -> Result<(String, Option<&str>), String> {
    let key = unquote(line[..index].trim())?;
    let value = line[index + 1..].trim();
    if value.is_empty() {
        Ok((key, None))
    } else {
        Ok((key, Some(value)))
    }
}

fn parse_scalar_or_inline(value: &str) -> Result<Yaml, String> {
    let value = value.trim();
    if let Some(inner) = inline_map_body(value) {
        return parse_inline_map(inner);
    }
    Ok(Yaml::Scalar(unquote(value)?))
}

fn inline_map_body(value: &str) -> Option<&str> {
    value
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
}

fn parse_inline_map(inner: &str) -> Result<Yaml, String> {
    let mut map = BTreeMap::new();
    if inner.trim().is_empty() {
        return Ok(Yaml::Map(map));
    }
    for part in split_inline(inner)? {
        insert_inline_part(&mut map, part)?;
    }
    Ok(Yaml::Map(map))
}

fn insert_inline_part(map: &mut BTreeMap<String, Yaml>, part: &str) -> Result<(), String> {
    let (key, raw) = split_entry(part.trim())?;
    let Some(raw) = raw else {
        return Err(format!("inline pnpm value is incomplete: {part}"));
    };
    map.insert(key, Yaml::Scalar(unquote(raw)?));
    Ok(())
}

fn split_inline(value: &str) -> Result<Vec<&str>, String> {
    let mut parts = Vec::new();
    let mut cursor = InlineCursor::default();
    for (index, character) in value.char_indices() {
        take_inline_char(value, index, character, &mut cursor, &mut parts);
    }
    let tail = value[cursor.start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    Ok(parts)
}

#[derive(Default)]
struct InlineCursor {
    start: usize,
    quoted: Option<char>,
    depth: i32,
}

fn take_inline_char<'a>(
    value: &'a str,
    index: usize,
    character: char,
    cursor: &mut InlineCursor,
    parts: &mut Vec<&'a str>,
) {
    if inline_separator(cursor, character) {
        parts.push(value[cursor.start..index].trim());
        cursor.start = index + 1;
        return;
    }
    update_inline_quote(cursor, character);
}

fn inline_separator(cursor: &InlineCursor, character: char) -> bool {
    cursor.quoted.is_none() && character == ',' && cursor.depth == 0
}

fn update_inline_quote(cursor: &mut InlineCursor, character: char) {
    match (cursor.quoted, character) {
        (None, '\'' | '"') => cursor.quoted = Some(character),
        (Some(quote), character) if character == quote => cursor.quoted = None,
        (None, '{') => cursor.depth += 1,
        (None, '}') => cursor.depth -= 1,
        _ => {}
    }
}

fn unquote(value: &str) -> Result<String, String> {
    let value = value.trim();
    if let Some(inner) = value
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
    {
        return Ok(inner.replace("''", "'"));
    }
    if let Some(inner) = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    {
        return Ok(inner.replace("\\\"", "\""));
    }
    if value.starts_with('\'') || value.starts_with('"') {
        return Err(format!("unterminated quoted pnpm value: {value}"));
    }
    Ok(value.to_owned())
}

fn parse_error(message: String) -> JavascriptError {
    JavascriptError::LockfileParse {
        path: RepoPath::parse("pnpm-lock.yaml").expect("static path"),
        message,
    }
}
