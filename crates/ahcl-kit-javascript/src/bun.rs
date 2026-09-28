// crates/ahcl-kit-javascript/src/bun.rs - bun.lock text resolution.
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

use crate::adapter::{json_string, manifest_value, string_map};
use crate::error::JavascriptError;
use crate::model::{ParsedEdge, ParsedGraph, ParsedPackage, split_package_ident};
use ahcl_kit_core::{DependencyKind, LockfileEvidence, RepoPath};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

struct BunPackage {
    id: String,
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
    workspace: bool,
    dependencies: BTreeMap<String, String>,
    dev_dependencies: BTreeMap<String, String>,
}

pub(crate) fn parse_bun(
    manifest_text: &str,
    lock_text: &str,
    manifest_path: &Path,
    lockfile: LockfileEvidence,
) -> Result<ParsedGraph, JavascriptError> {
    let manifest = manifest_value(manifest_text).map_err(parse_error)?;
    let lock = parsed_bun_lock(lock_text)?;
    let nodes = bun_nodes(&lock)?;
    let mut graph = ParsedGraph {
        packages: Vec::new(),
        edges: Vec::new(),
    };
    push_bun_workspaces(
        &manifest,
        &lock,
        &nodes,
        manifest_path,
        &lockfile,
        &mut graph,
    )?;
    ensure_bun_workspace(&graph)?;
    push_bun_packages(&nodes, manifest_path, &lockfile, &mut graph)?;
    Ok(graph)
}

fn parsed_bun_lock(lock_text: &str) -> Result<Value, JavascriptError> {
    serde_json::from_str(&strip_jsonc(lock_text)).map_err(|error| parse_error(error.to_string()))
}

fn bun_nodes(lock: &Value) -> Result<BTreeMap<String, BunPackage>, JavascriptError> {
    let packages = lock
        .get("packages")
        .and_then(Value::as_object)
        .ok_or_else(|| parse_error("bun lockfile is missing packages".to_owned()))?;
    let mut nodes = BTreeMap::new();
    for (key, value) in packages {
        nodes.insert(key.clone(), bun_package(key, value)?);
    }
    Ok(nodes)
}

fn push_bun_workspaces(
    manifest: &Value,
    lock: &Value,
    nodes: &BTreeMap<String, BunPackage>,
    manifest_path: &Path,
    lockfile: &LockfileEvidence,
    graph: &mut ParsedGraph,
) -> Result<(), JavascriptError> {
    let Some(workspaces) = lock.get("workspaces").and_then(Value::as_object) else {
        return Ok(());
    };
    for (path, workspace) in workspaces {
        push_bun_workspace(
            &BunEmit {
                manifest,
                nodes,
                manifest_path,
                lockfile,
            },
            path,
            workspace,
            graph,
        )?;
    }
    Ok(())
}

struct BunEmit<'a> {
    manifest: &'a Value,
    nodes: &'a BTreeMap<String, BunPackage>,
    manifest_path: &'a Path,
    lockfile: &'a LockfileEvidence,
}

fn push_bun_workspace(
    emit: &BunEmit<'_>,
    path: &str,
    workspace: &Value,
    graph: &mut ParsedGraph,
) -> Result<(), JavascriptError> {
    let name = bun_workspace_name(path, workspace, emit.manifest);
    let version = bun_workspace_version(path, workspace, emit.manifest);
    let id = format!("bun:workspace:{path}:{name}@{version}");
    push_workspace_dependency_edges(&id, path, workspace, emit.nodes, &mut graph.edges)?;
    graph.packages.push(ParsedPackage {
        id,
        name,
        version,
        source: Some("workspace".to_owned()),
        checksum: None,
        declared_license: bun_workspace_license(path, workspace, emit.manifest),
        workspace_root: true,
        manifest_path: emit.manifest_path.to_path_buf(),
        lockfiles: vec![emit.lockfile.clone()],
    });
    Ok(())
}

fn bun_workspace_name(path: &str, workspace: &Value, manifest: &Value) -> String {
    json_string(workspace, "name")
        .or_else(|| empty_path_field(path, manifest, "name"))
        .unwrap_or_else(|| fallback_workspace_name(path))
}

fn bun_workspace_version(path: &str, workspace: &Value, manifest: &Value) -> String {
    json_string(workspace, "version")
        .or_else(|| empty_path_field(path, manifest, "version"))
        .unwrap_or_else(|| "0.0.0".to_owned())
}

fn bun_workspace_license(path: &str, workspace: &Value, manifest: &Value) -> Option<String> {
    json_string(workspace, "license").or_else(|| empty_path_field(path, manifest, "license"))
}

fn empty_path_field(path: &str, manifest: &Value, key: &str) -> Option<String> {
    path.is_empty()
        .then(|| json_string(manifest, key))
        .flatten()
}

fn fallback_workspace_name(path: &str) -> String {
    if path.is_empty() {
        "workspace".to_owned()
    } else {
        path.to_owned()
    }
}

fn push_workspace_dependency_edges(
    id: &str,
    path: &str,
    workspace: &Value,
    nodes: &BTreeMap<String, BunPackage>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    for (key, kind, targets) in workspace_dependency_groups() {
        let edge = WorkspaceEdge {
            id,
            path,
            key,
            kind,
            targets: &targets,
            nodes,
        };
        push_workspace_group(&edge, workspace, edges)?;
    }
    Ok(())
}

struct WorkspaceEdge<'a> {
    id: &'a str,
    path: &'a str,
    key: &'a str,
    kind: DependencyKind,
    targets: &'a [String],
    nodes: &'a BTreeMap<String, BunPackage>,
}

fn workspace_dependency_groups() -> [(&'static str, DependencyKind, Vec<String>); 3] {
    [
        ("dependencies", DependencyKind::Normal, Vec::new()),
        (
            "optionalDependencies",
            DependencyKind::Normal,
            vec!["optional".to_owned()],
        ),
        ("devDependencies", DependencyKind::Development, Vec::new()),
    ]
}

fn push_workspace_group(
    edge: &WorkspaceEdge<'_>,
    workspace: &Value,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    for (name, _) in string_map(workspace, edge.key) {
        push_workspace_edge(edge, &name, edges)?;
    }
    Ok(())
}

fn push_workspace_edge(
    edge: &WorkspaceEdge<'_>,
    name: &str,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    let Some(package) = edge.nodes.get(name) else {
        return missing_workspace_dependency(edge, name);
    };
    edges.push(ParsedEdge {
        from: edge.id.to_owned(),
        to: package.id.clone(),
        kind: edge.kind,
        targets: edge.targets.to_vec(),
    });
    Ok(())
}

fn missing_workspace_dependency(
    edge: &WorkspaceEdge<'_>,
    name: &str,
) -> Result<(), JavascriptError> {
    if edge.key == "optionalDependencies" {
        return Ok(());
    }
    Err(parse_error(format!(
        "bun workspace {} depends on unresolved {name}",
        edge.path
    )))
}

fn ensure_bun_workspace(graph: &ParsedGraph) -> Result<(), JavascriptError> {
    if graph.packages.iter().any(|package| package.workspace_root) {
        Ok(())
    } else {
        Err(parse_error("bun lockfile has no workspace".to_owned()))
    }
}

fn push_bun_packages(
    nodes: &BTreeMap<String, BunPackage>,
    manifest_path: &Path,
    lockfile: &LockfileEvidence,
    graph: &mut ParsedGraph,
) -> Result<(), JavascriptError> {
    for package in nodes.values() {
        push_bun_package_edges(package, nodes, &mut graph.edges)?;
        graph
            .packages
            .push(parsed_bun_package(package, manifest_path, lockfile));
    }
    Ok(())
}

fn push_bun_package_edges(
    package: &BunPackage,
    nodes: &BTreeMap<String, BunPackage>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    push_named_edges(
        package,
        package.dependencies.keys(),
        DependencyKind::Normal,
        nodes,
        edges,
    )?;
    push_named_edges(
        package,
        package.dev_dependencies.keys(),
        DependencyKind::Development,
        nodes,
        edges,
    )
}

fn push_named_edges<'a>(
    package: &BunPackage,
    names: impl Iterator<Item = &'a String>,
    kind: DependencyKind,
    nodes: &BTreeMap<String, BunPackage>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    for name in names {
        let Some(target) = nodes.get(name) else {
            return Err(parse_error(format!(
                "bun package {} depends on unresolved {name}",
                package.name
            )));
        };
        edges.push(ParsedEdge {
            from: package.id.clone(),
            to: target.id.clone(),
            kind,
            targets: Vec::new(),
        });
    }
    Ok(())
}

fn parsed_bun_package(
    package: &BunPackage,
    manifest_path: &Path,
    lockfile: &LockfileEvidence,
) -> ParsedPackage {
    ParsedPackage {
        id: package.id.clone(),
        name: package.name.clone(),
        version: package.version.clone(),
        source: package.source.clone(),
        checksum: package.checksum.clone(),
        declared_license: None,
        workspace_root: package.workspace,
        manifest_path: manifest_path.to_path_buf(),
        lockfiles: vec![lockfile.clone()],
    }
}

fn bun_package(key: &str, value: &Value) -> Result<BunPackage, JavascriptError> {
    let tuple = value
        .as_array()
        .ok_or_else(|| parse_error(format!("bun package {key} is not an array")))?;
    let ident = tuple
        .first()
        .and_then(Value::as_str)
        .ok_or_else(|| parse_error(format!("bun package {key} has no identity")))?;
    let (name, version) = split_package_ident(ident)
        .ok_or_else(|| parse_error(format!("bun package identity is invalid: {ident}")))?;
    let source = tuple
        .get(1)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let meta = tuple.get(2);
    let checksum = tuple.get(3).and_then(Value::as_str).map(str::to_owned);
    let workspace = ident.contains("@workspace:");
    Ok(BunPackage {
        id: format!("bun:{key}@{version}"),
        name: name.to_owned(),
        version: version.to_owned(),
        source: source.map(str::to_owned),
        checksum,
        workspace,
        dependencies: meta
            .map(|meta| string_map(meta, "dependencies"))
            .unwrap_or_default(),
        dev_dependencies: meta
            .map(|meta| string_map(meta, "devDependencies"))
            .unwrap_or_default(),
    })
}

fn strip_jsonc(text: &str) -> String {
    // Comment markers inside JSON strings stay in the text. Only markers
    // outside strings are comments.
    let chars = text.chars().collect::<Vec<_>>();
    let mut stripped = String::with_capacity(text.len());
    let mut index = 0;
    let mut state = StringScan::default();
    while index < chars.len() {
        if state.string {
            index = push_string_char(&chars, index, &mut stripped, &mut state);
            continue;
        }
        if chars[index] == '"' {
            state.string = true;
            stripped.push('"');
            index += 1;
            continue;
        }
        if starts_line_comment(&chars, index) {
            index = skip_line_comment(&chars, index);
            continue;
        }
        if starts_block_comment(&chars, index) {
            index = skip_block_comment(&chars, index);
            continue;
        }
        stripped.push(chars[index]);
        index += 1;
    }
    strip_trailing_commas(&stripped)
}

#[derive(Default)]
struct StringScan {
    escaped: bool,
    string: bool,
}

fn push_string_char(
    chars: &[char],
    index: usize,
    stripped: &mut String,
    state: &mut StringScan,
) -> usize {
    let character = chars[index];
    stripped.push(character);
    advance_string(state, character);
    index + 1
}

fn advance_string(state: &mut StringScan, character: char) {
    if state.escaped {
        state.escaped = false;
        return;
    }
    state.escaped = character == '\\';
    if character == '"' {
        state.string = false;
    }
}

fn starts_line_comment(chars: &[char], index: usize) -> bool {
    chars[index] == '/' && chars.get(index + 1) == Some(&'/')
}

fn skip_line_comment(chars: &[char], index: usize) -> usize {
    let mut index = index + 2;
    while index < chars.len() && chars[index] != '\n' {
        index += 1;
    }
    index
}

fn starts_block_comment(chars: &[char], index: usize) -> bool {
    chars[index] == '/' && chars.get(index + 1) == Some(&'*')
}

fn skip_block_comment(chars: &[char], index: usize) -> usize {
    let mut index = index + 2;
    while block_comment_continues(chars, index) {
        index += 1;
    }
    (index + 2).min(chars.len())
}

fn block_comment_continues(chars: &[char], index: usize) -> bool {
    index + 1 < chars.len() && !block_comment_ends(chars, index)
}

fn block_comment_ends(chars: &[char], index: usize) -> bool {
    chars[index] == '*' && chars[index + 1] == '/'
}

fn strip_trailing_commas(text: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let mut stripped = String::with_capacity(text.len());
    let mut index = 0;
    let mut state = StringScan::default();
    while index < chars.len() {
        if state.string {
            index = push_string_char(&chars, index, &mut stripped, &mut state);
            continue;
        }
        if chars[index] == '"' {
            state.string = true;
            stripped.push('"');
            index += 1;
            continue;
        }
        if chars[index] == ',' && next_nonspace_closes(&chars, index) {
            index += 1;
            continue;
        }
        stripped.push(chars[index]);
        index += 1;
    }
    stripped
}

fn next_nonspace_closes(chars: &[char], index: usize) -> bool {
    let mut look = index + 1;
    while look < chars.len() && chars[look].is_whitespace() {
        look += 1;
    }
    matches!(chars.get(look), Some('}' | ']'))
}

fn parse_error(message: String) -> JavascriptError {
    JavascriptError::LockfileParse {
        path: RepoPath::parse("bun.lock").expect("static path"),
        message,
    }
}
