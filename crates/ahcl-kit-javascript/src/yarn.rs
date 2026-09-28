// crates/ahcl-kit-javascript/src/yarn.rs - yarn.lock resolution for classic and Berry lockfiles.
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
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug)]
struct YarnEntry {
    descriptors: Vec<String>,
    version: String,
    resolved: Option<String>,
    integrity: Option<String>,
    dependencies: BTreeMap<String, String>,
    dev_dependencies: BTreeMap<String, String>,
    workspace: bool,
}

pub(crate) fn parse_yarn(
    manifest_text: &str,
    lock_text: &str,
    manifest_path: &Path,
    lockfile: LockfileEvidence,
) -> Result<ParsedGraph, JavascriptError> {
    let manifest = manifest_value(manifest_text).map_err(parse_error)?;
    if is_berry(lock_text) {
        parse_berry(&manifest, lock_text, manifest_path, lockfile)
    } else {
        parse_classic(&manifest, lock_text, manifest_path, lockfile)
    }
}

fn is_berry(lock_text: &str) -> bool {
    lock_text.lines().any(|line| line.trim() == "__metadata:")
}

fn parse_classic(
    manifest: &serde_json::Value,
    lock_text: &str,
    manifest_path: &Path,
    lockfile: LockfileEvidence,
) -> Result<ParsedGraph, JavascriptError> {
    let entries = parse_classic_entries(lock_text)?;
    let mut graph = classic_root(manifest, manifest_path, &lockfile);
    let by_descriptor = index_classic_packages(&entries, &mut graph, manifest_path, &lockfile)?;
    let root_id = graph.packages[0].id.clone();
    push_manifest_edges(manifest, &root_id, &by_descriptor, &mut graph.edges)?;
    push_classic_entry_edges(&entries, &by_descriptor, &mut graph.edges)?;
    Ok(graph)
}

fn classic_root(
    manifest: &serde_json::Value,
    manifest_path: &Path,
    lockfile: &LockfileEvidence,
) -> ParsedGraph {
    let name = json_string(manifest, "name").unwrap_or_else(|| "workspace".to_owned());
    let version = json_string(manifest, "version").unwrap_or_else(|| "0.0.0".to_owned());
    ParsedGraph {
        packages: vec![ParsedPackage {
            id: format!("yarn:{name}@{version}"),
            name,
            version,
            source: Some("workspace".to_owned()),
            checksum: None,
            declared_license: json_string(manifest, "license"),
            workspace_root: true,
            manifest_path: manifest_path.to_path_buf(),
            lockfiles: vec![lockfile.clone()],
        }],
        edges: Vec::new(),
    }
}

fn index_classic_packages(
    entries: &[YarnEntry],
    graph: &mut ParsedGraph,
    manifest_path: &Path,
    lockfile: &LockfileEvidence,
) -> Result<BTreeMap<String, String>, JavascriptError> {
    let mut by_descriptor = BTreeMap::new();
    for entry in entries {
        index_classic_entry(entry, graph, manifest_path, lockfile, &mut by_descriptor)?;
    }
    Ok(by_descriptor)
}

fn index_classic_entry(
    entry: &YarnEntry,
    graph: &mut ParsedGraph,
    manifest_path: &Path,
    lockfile: &LockfileEvidence,
    by_descriptor: &mut BTreeMap<String, String>,
) -> Result<(), JavascriptError> {
    let package_name = entry_name(&entry.descriptors)?;
    let id = format!("yarn:{package_name}@{}", entry.version);
    for descriptor in &entry.descriptors {
        by_descriptor.insert(descriptor.clone(), id.clone());
    }
    graph.packages.push(ParsedPackage {
        id,
        name: package_name,
        version: entry.version.clone(),
        source: entry.resolved.clone(),
        checksum: entry.integrity.clone(),
        declared_license: None,
        workspace_root: false,
        manifest_path: manifest_path.to_path_buf(),
        lockfiles: vec![lockfile.clone()],
    });
    Ok(())
}

fn push_classic_entry_edges(
    entries: &[YarnEntry],
    by_descriptor: &BTreeMap<String, String>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    for entry in entries {
        push_classic_entry_edge(entry, by_descriptor, edges)?;
    }
    Ok(())
}

fn push_classic_entry_edge(
    entry: &YarnEntry,
    by_descriptor: &BTreeMap<String, String>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    let from = by_descriptor
        .get(&entry.descriptors[0])
        .cloned()
        .ok_or_else(|| parse_error("yarn descriptor was not indexed".to_owned()))?;
    for (name, range) in &entry.dependencies {
        push_classic_dependency(&from, name, range, by_descriptor, edges)?;
    }
    Ok(())
}

fn push_classic_dependency(
    from: &str,
    name: &str,
    range: &str,
    by_descriptor: &BTreeMap<String, String>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    let descriptor = format!("{name}@{range}");
    let Some(to) = by_descriptor.get(&descriptor) else {
        return Err(parse_error(format!(
            "yarn dependency {descriptor} was not found"
        )));
    };
    edges.push(ParsedEdge {
        from: from.to_owned(),
        to: to.clone(),
        kind: DependencyKind::Normal,
        targets: Vec::new(),
    });
    Ok(())
}

fn parse_berry(
    manifest: &serde_json::Value,
    lock_text: &str,
    manifest_path: &Path,
    lockfile: LockfileEvidence,
) -> Result<ParsedGraph, JavascriptError> {
    let entries = parse_berry_entries(lock_text)?;
    let by_descriptor = index_berry_entries(entries);
    let emit = BerryEmit {
        manifest,
        manifest_path,
        lockfile: &lockfile,
    };
    let (packages, ids) = berry_packages(&by_descriptor, &emit)?;
    ensure_berry_workspace(&packages)?;
    Ok(ParsedGraph {
        packages,
        edges: berry_edges(&by_descriptor, &ids)?,
    })
}

fn index_berry_entries(entries: Vec<YarnEntry>) -> BTreeMap<String, YarnEntry> {
    let mut by_descriptor = BTreeMap::new();
    for entry in entries {
        for descriptor in &entry.descriptors {
            by_descriptor.insert(descriptor.clone(), entry.clone());
        }
    }
    by_descriptor
}

struct BerryEmit<'a> {
    manifest: &'a serde_json::Value,
    manifest_path: &'a Path,
    lockfile: &'a LockfileEvidence,
}

fn berry_packages(
    by_descriptor: &BTreeMap<String, YarnEntry>,
    emit: &BerryEmit<'_>,
) -> Result<(Vec<ParsedPackage>, BTreeMap<String, String>), JavascriptError> {
    let mut packages = Vec::new();
    let mut ids = BTreeMap::new();
    for (descriptor, entry) in by_descriptor {
        push_berry_package(&mut packages, &mut ids, descriptor, entry, emit)?;
    }
    Ok((packages, ids))
}

fn push_berry_package(
    packages: &mut Vec<ParsedPackage>,
    ids: &mut BTreeMap<String, String>,
    descriptor: &str,
    entry: &YarnEntry,
    emit: &BerryEmit<'_>,
) -> Result<(), JavascriptError> {
    let (name, _) = split_package_ident(descriptor)
        .ok_or_else(|| parse_error(format!("yarn descriptor is invalid: {descriptor}")))?;
    let id = berry_package_id(entry, emit.manifest, name);
    ids.insert(descriptor.to_owned(), id.clone());
    if packages.iter().any(|package| package.id == id) {
        return Ok(());
    }
    packages.push(berry_parsed_package(entry, emit, name, id));
    Ok(())
}

fn berry_package_id(entry: &YarnEntry, manifest: &serde_json::Value, name: &str) -> String {
    if entry.workspace {
        let manifest_name = json_string(manifest, "name").unwrap_or_else(|| name.to_owned());
        return format!("yarn:{manifest_name}@{}", entry.version);
    }
    format!("yarn:{name}@{}", entry.version)
}

fn berry_parsed_package(
    entry: &YarnEntry,
    emit: &BerryEmit<'_>,
    name: &str,
    id: String,
) -> ParsedPackage {
    ParsedPackage {
        id,
        name: berry_package_name(entry, emit.manifest, name),
        version: entry.version.clone(),
        source: entry
            .resolved
            .clone()
            .or_else(|| entry.workspace.then(|| "workspace".to_owned())),
        checksum: entry.integrity.clone(),
        declared_license: entry
            .workspace
            .then(|| json_string(emit.manifest, "license"))
            .flatten(),
        workspace_root: entry.workspace,
        manifest_path: emit.manifest_path.to_path_buf(),
        lockfiles: vec![emit.lockfile.clone()],
    }
}

fn berry_package_name(entry: &YarnEntry, manifest: &serde_json::Value, name: &str) -> String {
    if entry.workspace {
        json_string(manifest, "name").unwrap_or_else(|| name.to_owned())
    } else {
        name.to_owned()
    }
}

fn ensure_berry_workspace(packages: &[ParsedPackage]) -> Result<(), JavascriptError> {
    if packages.iter().any(|package| package.workspace_root) {
        Ok(())
    } else {
        Err(parse_error(
            "yarn berry lockfile has no workspace package".to_owned(),
        ))
    }
}

fn berry_edges(
    by_descriptor: &BTreeMap<String, YarnEntry>,
    ids: &BTreeMap<String, String>,
) -> Result<Vec<ParsedEdge>, JavascriptError> {
    let mut edges = Vec::new();
    for (descriptor, entry) in by_descriptor {
        push_berry_entry_edges(&ids[descriptor], entry, ids, &mut edges)?;
    }
    Ok(edges)
}

fn push_berry_entry_edges(
    from: &str,
    entry: &YarnEntry,
    ids: &BTreeMap<String, String>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    push_berry_dependencies(
        from,
        &entry.dependencies,
        DependencyKind::Normal,
        ids,
        edges,
    )?;
    push_berry_dependencies(
        from,
        &entry.dev_dependencies,
        DependencyKind::Development,
        ids,
        edges,
    )
}

fn push_berry_dependencies(
    from: &str,
    dependencies: &BTreeMap<String, String>,
    kind: DependencyKind,
    ids: &BTreeMap<String, String>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    for (name, range) in dependencies {
        push_berry_dependency(from, name, range, kind, ids, edges)?;
    }
    Ok(())
}

fn push_berry_dependency(
    from: &str,
    name: &str,
    range: &str,
    kind: DependencyKind,
    ids: &BTreeMap<String, String>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    let descriptor = format!("{name}@{range}");
    let Some(to) = ids.get(&descriptor) else {
        return Err(parse_error(format!(
            "yarn dependency {descriptor} was not found"
        )));
    };
    edges.push(ParsedEdge {
        from: from.to_owned(),
        to: to.clone(),
        kind,
        targets: Vec::new(),
    });
    Ok(())
}

fn push_manifest_edges(
    manifest: &serde_json::Value,
    root_id: &str,
    by_descriptor: &BTreeMap<String, String>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    for (key, kind) in [
        ("dependencies", DependencyKind::Normal),
        ("devDependencies", DependencyKind::Development),
        ("optionalDependencies", DependencyKind::Normal),
    ] {
        for (name, range) in string_map(manifest, key) {
            let descriptor = format!("{name}@{range}");
            let Some(to) = by_descriptor.get(&descriptor) else {
                return Err(parse_error(format!(
                    "yarn dependency {descriptor} was not found"
                )));
            };
            let mut targets = Vec::new();
            if key == "optionalDependencies" {
                targets.push("optional".to_owned());
            }
            edges.push(ParsedEdge {
                from: root_id.to_owned(),
                to: to.clone(),
                kind,
                targets,
            });
        }
    }
    Ok(())
}

fn parse_classic_entries(text: &str) -> Result<Vec<YarnEntry>, JavascriptError> {
    let mut state = ClassicParse {
        entries: Vec::new(),
        current: None,
        in_dependencies: false,
    };
    for line in text.lines() {
        push_classic_line(&mut state, line.trim_end_matches('\r'))?;
    }
    finish_classic_entries(state)
}

struct ClassicParse {
    entries: Vec<YarnEntry>,
    current: Option<YarnEntry>,
    in_dependencies: bool,
}

fn push_classic_line(state: &mut ClassicParse, line: &str) -> Result<(), JavascriptError> {
    if yarn_line_ignored(line) {
        return Ok(());
    }
    if yarn_header(line) {
        return start_classic_entry(state, line);
    }
    append_classic_field(state, line)
}

fn yarn_line_ignored(line: &str) -> bool {
    line.trim().is_empty() || line.trim_start().starts_with('#')
}

fn yarn_header(line: &str) -> bool {
    !line.starts_with(' ') && line.ends_with(':')
}

fn start_classic_entry(state: &mut ClassicParse, line: &str) -> Result<(), JavascriptError> {
    if let Some(entry) = state.current.take() {
        state.entries.push(entry);
    }
    state.current = Some(empty_yarn_entry(split_descriptors(
        &line[..line.len() - 1],
    )?));
    state.in_dependencies = false;
    Ok(())
}

fn empty_yarn_entry(descriptors: Vec<String>) -> YarnEntry {
    YarnEntry {
        descriptors,
        version: String::new(),
        resolved: None,
        integrity: None,
        dependencies: BTreeMap::new(),
        dev_dependencies: BTreeMap::new(),
        workspace: false,
    }
}

fn append_classic_field(state: &mut ClassicParse, line: &str) -> Result<(), JavascriptError> {
    ensure_classic_entry(state, line)?;
    close_classic_dependencies(state, leading_spaces(line));
    if line.trim() == "dependencies:" {
        state.in_dependencies = true;
        return Ok(());
    }
    // Copy the dependency flag before the entry borrow begins.
    write_classic_field(state, line)
}

fn ensure_classic_entry(state: &ClassicParse, line: &str) -> Result<(), JavascriptError> {
    if state.current.is_some() {
        return Ok(());
    }
    Err(parse_error(format!(
        "yarn lockfile line is outside an entry: {line}"
    )))
}

fn close_classic_dependencies(state: &mut ClassicParse, indent: usize) {
    if state.in_dependencies && indent <= 2 {
        state.in_dependencies = false;
    }
}

fn write_classic_field(state: &mut ClassicParse, line: &str) -> Result<(), JavascriptError> {
    let in_dependencies = state.in_dependencies;
    let entry = current_classic_entry(state, line)?;
    if in_dependencies {
        return insert_classic_dependency(entry, line.trim());
    }
    assign_classic_field(entry, line.trim())
}

fn current_classic_entry<'a>(
    state: &'a mut ClassicParse,
    line: &str,
) -> Result<&'a mut YarnEntry, JavascriptError> {
    state
        .current
        .as_mut()
        .ok_or_else(|| parse_error(format!("yarn lockfile line is outside an entry: {line}")))
}

fn leading_spaces(line: &str) -> usize {
    line.chars()
        .take_while(|character| *character == ' ')
        .count()
}

fn insert_classic_dependency(entry: &mut YarnEntry, trimmed: &str) -> Result<(), JavascriptError> {
    let (name, range) = split_field(trimmed)?;
    entry.dependencies.insert(name, unquote(&range));
    Ok(())
}

fn assign_classic_field(entry: &mut YarnEntry, trimmed: &str) -> Result<(), JavascriptError> {
    let (key, value) = split_field(trimmed)?;
    match key.as_str() {
        "version" => entry.version = unquote(&value),
        "resolved" => entry.resolved = Some(unquote(&value)),
        "integrity" => entry.integrity = Some(unquote(&value)),
        _ => {}
    }
    Ok(())
}

fn finish_classic_entries(state: ClassicParse) -> Result<Vec<YarnEntry>, JavascriptError> {
    let mut entries = state.entries;
    if let Some(entry) = state.current {
        entries.push(entry);
    }
    if entries.iter().any(|entry| entry.version.is_empty()) {
        return Err(parse_error("yarn entry is missing a version".to_owned()));
    }
    Ok(entries)
}

fn parse_berry_entries(text: &str) -> Result<Vec<YarnEntry>, JavascriptError> {
    let mut state = BerryParse {
        entries: Vec::new(),
        current: None,
        section: String::new(),
    };
    for line in text.lines() {
        push_berry_line(&mut state, line.trim_end_matches('\r'))?;
    }
    Ok(finish_berry_entries(state))
}

struct BerryParse {
    entries: Vec<YarnEntry>,
    current: Option<YarnEntry>,
    section: String,
}

fn push_berry_line(state: &mut BerryParse, line: &str) -> Result<(), JavascriptError> {
    if yarn_line_ignored(line) {
        return Ok(());
    }
    if yarn_header(line) {
        return start_berry_entry(state, line);
    }
    append_berry_field(state, line)
}

fn start_berry_entry(state: &mut BerryParse, line: &str) -> Result<(), JavascriptError> {
    if let Some(entry) = state.current.take() {
        push_berry_entry(&mut state.entries, entry);
    }
    let descriptors = berry_descriptors(line[..line.len() - 1].trim())?;
    state.current = Some(empty_yarn_entry(descriptors));
    state.section.clear();
    Ok(())
}

fn push_berry_entry(entries: &mut Vec<YarnEntry>, entry: YarnEntry) {
    if entry.descriptors != ["__metadata"] {
        entries.push(entry);
    }
}

fn append_berry_field(state: &mut BerryParse, line: &str) -> Result<(), JavascriptError> {
    let Some(entry) = state.current.as_mut() else {
        return Ok(());
    };
    if !state.section.is_empty() && leading_spaces(line) <= 2 {
        state.section.clear();
    }
    let trimmed = line.trim();
    if berry_section_header(trimmed) {
        state.section = berry_section_name(trimmed).to_owned();
        return Ok(());
    }
    assign_berry_field(entry, &state.section, trimmed)
}

fn berry_section_header(trimmed: &str) -> bool {
    trimmed.ends_with(':') && !trimmed.contains(' ')
}

fn berry_section_name(trimmed: &str) -> &'static str {
    match trimmed.trim_end_matches(':') {
        "dependencies" => "dependencies",
        "devDependencies" => "devDependencies",
        _ => "",
    }
}

fn assign_berry_field(
    entry: &mut YarnEntry,
    section: &str,
    trimmed: &str,
) -> Result<(), JavascriptError> {
    let (key, value) = split_field(trimmed)?;
    let value = unquote(&value);
    if section == "dependencies" {
        entry.dependencies.insert(key, value);
        return Ok(());
    }
    if section == "devDependencies" {
        entry.dev_dependencies.insert(key, value);
        return Ok(());
    }
    assign_berry_scalar(entry, &key, value);
    Ok(())
}

fn assign_berry_scalar(entry: &mut YarnEntry, key: &str, value: String) {
    match key {
        "version" => entry.version = value,
        "resolution" => assign_berry_resolution(entry, value),
        "checksum" => entry.integrity = Some(value),
        _ => {}
    }
}

fn assign_berry_resolution(entry: &mut YarnEntry, value: String) {
    entry.workspace = value.contains("@workspace:");
    entry.resolved = (!entry.workspace).then_some(value);
}

fn finish_berry_entries(state: BerryParse) -> Vec<YarnEntry> {
    let mut entries = state.entries;
    if let Some(entry) = state.current {
        push_berry_entry(&mut entries, entry);
    }
    entries
}

fn entry_name(descriptors: &[String]) -> Result<String, JavascriptError> {
    let descriptor = descriptors
        .first()
        .ok_or_else(|| parse_error("yarn entry has no descriptor".to_owned()))?;
    split_package_ident(descriptor)
        .map(|(name, _)| name.to_owned())
        .ok_or_else(|| parse_error(format!("yarn descriptor is invalid: {descriptor}")))
}

fn berry_descriptors(header: &str) -> Result<Vec<String>, JavascriptError> {
    let header = header.trim();
    let inner = if header.len() >= 2
        && ((header.starts_with('"') && header.ends_with('"'))
            || (header.starts_with('\'') && header.ends_with('\'')))
    {
        &header[1..header.len() - 1]
    } else {
        header
    };
    split_descriptors(inner)
}

fn split_descriptors(header: &str) -> Result<Vec<String>, JavascriptError> {
    let mut descriptors = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in header.chars() {
        take_descriptor_char(character, &mut quoted, &mut current, &mut descriptors);
    }
    push_descriptor(&mut descriptors, &current);
    if descriptors.is_empty() {
        return Err(parse_error(format!("yarn header is empty: {header}")));
    }
    Ok(descriptors)
}

fn take_descriptor_char(
    character: char,
    quoted: &mut bool,
    current: &mut String,
    descriptors: &mut Vec<String>,
) {
    match character {
        '"' => *quoted = !*quoted,
        ',' if !*quoted => {
            push_descriptor(descriptors, current);
            current.clear();
        }
        _ => current.push(character),
    }
}

fn push_descriptor(descriptors: &mut Vec<String>, current: &str) {
    let descriptor = current.trim().trim_matches('"').to_owned();
    if !descriptor.is_empty() {
        descriptors.push(descriptor);
    }
}

fn split_field(line: &str) -> Result<(String, String), JavascriptError> {
    let Some((key, value)) = line.split_once([' ', ':']) else {
        return Err(parse_error(format!("yarn field is invalid: {line}")));
    };
    let key = key.trim().trim_end_matches(':').trim_matches('"');
    Ok((key.to_owned(), value.trim().to_owned()))
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches('"').trim_matches('\'').to_owned()
}

fn parse_error(message: String) -> JavascriptError {
    JavascriptError::LockfileParse {
        path: RepoPath::parse("yarn.lock").expect("static path"),
        message,
    }
}
