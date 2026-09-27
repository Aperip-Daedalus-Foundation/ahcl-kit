// crates/ahcl-kit-javascript/src/yarn.rs - yarn.lock resolution for classic and Berry lockfiles.
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
    let name = json_string(manifest, "name").unwrap_or_else(|| "workspace".to_owned());
    let version = json_string(manifest, "version").unwrap_or_else(|| "0.0.0".to_owned());
    let root_id = format!("yarn:{name}@{version}");
    let mut by_descriptor = BTreeMap::<String, String>::new();
    let mut packages = vec![ParsedPackage {
        id: root_id.clone(),
        name,
        version,
        source: Some("workspace".to_owned()),
        checksum: None,
        declared_license: json_string(manifest, "license"),
        workspace_root: true,
        manifest_path: manifest_path.to_path_buf(),
        lockfile: lockfile.clone(),
    }];
    let mut edges = Vec::new();
    for entry in &entries {
        let package_name = entry_name(&entry.descriptors)?;
        let id = format!("yarn:{package_name}@{}", entry.version);
        for descriptor in &entry.descriptors {
            by_descriptor.insert(descriptor.clone(), id.clone());
        }
        packages.push(ParsedPackage {
            id,
            name: package_name,
            version: entry.version.clone(),
            source: entry.resolved.clone(),
            checksum: entry.integrity.clone(),
            declared_license: None,
            workspace_root: false,
            manifest_path: manifest_path.to_path_buf(),
            lockfile: lockfile.clone(),
        });
    }
    push_manifest_edges(manifest, &root_id, &by_descriptor, &mut edges)?;
    for entry in &entries {
        let from = by_descriptor
            .get(&entry.descriptors[0])
            .cloned()
            .ok_or_else(|| parse_error("yarn descriptor was not indexed".to_owned()))?;
        for (name, range) in &entry.dependencies {
            let descriptor = format!("{name}@{range}");
            let Some(to) = by_descriptor.get(&descriptor) else {
                return Err(parse_error(format!(
                    "yarn dependency {descriptor} was not found"
                )));
            };
            edges.push(ParsedEdge {
                from: from.clone(),
                to: to.clone(),
                kind: DependencyKind::Normal,
                targets: Vec::new(),
            });
        }
    }
    Ok(ParsedGraph { packages, edges })
}

fn parse_berry(
    manifest: &serde_json::Value,
    lock_text: &str,
    manifest_path: &Path,
    lockfile: LockfileEvidence,
) -> Result<ParsedGraph, JavascriptError> {
    let entries = parse_berry_entries(lock_text)?;
    let mut by_descriptor = BTreeMap::<String, YarnEntry>::new();
    for entry in entries {
        for descriptor in &entry.descriptors {
            by_descriptor.insert(descriptor.clone(), entry.clone());
        }
    }
    let mut packages = Vec::new();
    let mut ids = BTreeMap::<String, String>::new();
    for (descriptor, entry) in &by_descriptor {
        let (name, _) = split_package_ident(descriptor)
            .ok_or_else(|| parse_error(format!("yarn descriptor is invalid: {descriptor}")))?;
        let id = if entry.workspace {
            let manifest_name = json_string(manifest, "name").unwrap_or_else(|| name.to_owned());
            format!("yarn:{manifest_name}@{}", entry.version)
        } else {
            format!("yarn:{name}@{}", entry.version)
        };
        ids.insert(descriptor.clone(), id.clone());
        if packages
            .iter()
            .any(|package: &ParsedPackage| package.id == id)
        {
            continue;
        }
        packages.push(ParsedPackage {
            id,
            name: if entry.workspace {
                json_string(manifest, "name").unwrap_or_else(|| name.to_owned())
            } else {
                name.to_owned()
            },
            version: entry.version.clone(),
            source: entry
                .resolved
                .clone()
                .or_else(|| entry.workspace.then(|| "workspace".to_owned())),
            checksum: entry.integrity.clone(),
            declared_license: entry
                .workspace
                .then(|| json_string(manifest, "license"))
                .flatten(),
            workspace_root: entry.workspace,
            manifest_path: manifest_path.to_path_buf(),
            lockfile: lockfile.clone(),
        });
    }
    if !packages.iter().any(|package| package.workspace_root) {
        return Err(parse_error(
            "yarn berry lockfile has no workspace package".to_owned(),
        ));
    }
    let mut edges = Vec::new();
    for (descriptor, entry) in &by_descriptor {
        let from = ids[descriptor].clone();
        for (dependencies, kind) in [
            (&entry.dependencies, DependencyKind::Normal),
            (&entry.dev_dependencies, DependencyKind::Development),
        ] {
            for (name, range) in dependencies {
                let dependency_descriptor = format!("{name}@{range}");
                let Some(to) = ids.get(&dependency_descriptor) else {
                    return Err(parse_error(format!(
                        "yarn dependency {dependency_descriptor} was not found"
                    )));
                };
                edges.push(ParsedEdge {
                    from: from.clone(),
                    to: to.clone(),
                    kind,
                    targets: Vec::new(),
                });
            }
        }
    }
    Ok(ParsedGraph { packages, edges })
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
    let mut entries = Vec::new();
    let mut current: Option<YarnEntry> = None;
    let mut in_dependencies = false;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') && line.ends_with(':') {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(YarnEntry {
                descriptors: split_descriptors(&line[..line.len() - 1])?,
                version: String::new(),
                resolved: None,
                integrity: None,
                dependencies: BTreeMap::new(),
                dev_dependencies: BTreeMap::new(),
                workspace: false,
            });
            in_dependencies = false;
            continue;
        }
        let Some(entry) = current.as_mut() else {
            return Err(parse_error(format!(
                "yarn lockfile line is outside an entry: {line}"
            )));
        };
        let indent = line
            .chars()
            .take_while(|character| *character == ' ')
            .count();
        if in_dependencies && indent <= 2 {
            in_dependencies = false;
        }
        let trimmed = line.trim();
        if trimmed == "dependencies:" {
            in_dependencies = true;
            continue;
        }
        if in_dependencies {
            let (name, range) = split_field(trimmed)?;
            entry.dependencies.insert(name, unquote(&range));
            continue;
        }
        let (key, value) = split_field(trimmed)?;
        match key.as_str() {
            "version" => entry.version = unquote(&value),
            "resolved" => entry.resolved = Some(unquote(&value)),
            "integrity" => entry.integrity = Some(unquote(&value)),
            _ => {}
        }
    }
    if let Some(entry) = current {
        entries.push(entry);
    }
    if entries.iter().any(|entry| entry.version.is_empty()) {
        return Err(parse_error("yarn entry is missing a version".to_owned()));
    }
    Ok(entries)
}

fn parse_berry_entries(text: &str) -> Result<Vec<YarnEntry>, JavascriptError> {
    let mut entries = Vec::new();
    let mut current: Option<YarnEntry> = None;
    let mut section = "";
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if !line.starts_with(' ') && line.ends_with(':') {
            if let Some(entry) = current.take() {
                if entry.descriptors != ["__metadata"] {
                    entries.push(entry);
                }
            }
            let header = unquote(line[..line.len() - 1].trim());
            current = Some(YarnEntry {
                descriptors: vec![header],
                version: String::new(),
                resolved: None,
                integrity: None,
                dependencies: BTreeMap::new(),
                dev_dependencies: BTreeMap::new(),
                workspace: false,
            });
            section = "";
            continue;
        }
        let Some(entry) = current.as_mut() else {
            continue;
        };
        let indent = line
            .chars()
            .take_while(|character| *character == ' ')
            .count();
        if !section.is_empty() && indent <= 2 {
            section = "";
        }
        let trimmed = line.trim();
        if trimmed.ends_with(':') && !trimmed.contains(' ') {
            section = match trimmed.trim_end_matches(':') {
                "dependencies" => "dependencies",
                "devDependencies" => "devDependencies",
                _ => "",
            };
            continue;
        }
        let (key, value) = split_field(trimmed)?;
        let value = unquote(&value);
        if section == "dependencies" {
            entry.dependencies.insert(key, value);
            continue;
        }
        if section == "devDependencies" {
            entry.dev_dependencies.insert(key, value);
            continue;
        }
        match key.as_str() {
            "version" => entry.version = value,
            "resolution" => {
                entry.workspace = value.contains("@workspace:");
                entry.resolved = (!entry.workspace).then_some(value);
            }
            "checksum" => entry.integrity = Some(value),
            _ => {}
        }
    }
    if let Some(entry) = current {
        if entry.descriptors != ["__metadata"] {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn entry_name(descriptors: &[String]) -> Result<String, JavascriptError> {
    let descriptor = descriptors
        .first()
        .ok_or_else(|| parse_error("yarn entry has no descriptor".to_owned()))?;
    split_package_ident(descriptor)
        .map(|(name, _)| name.to_owned())
        .ok_or_else(|| parse_error(format!("yarn descriptor is invalid: {descriptor}")))
}

fn split_descriptors(header: &str) -> Result<Vec<String>, JavascriptError> {
    let mut descriptors = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for character in header.chars() {
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                let descriptor = current.trim().trim_matches('"').to_owned();
                if !descriptor.is_empty() {
                    descriptors.push(descriptor);
                }
                current.clear();
            }
            _ => current.push(character),
        }
    }
    let descriptor = current.trim().trim_matches('"').to_owned();
    if !descriptor.is_empty() {
        descriptors.push(descriptor);
    }
    if descriptors.is_empty() {
        return Err(parse_error(format!("yarn header is empty: {header}")));
    }
    Ok(descriptors)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_classic_lockfile() {
        let manifest = r#"{"name":"demo","version":"0.1.0","dependencies":{"left-pad":"^1.3.0"},"devDependencies":{"typescript":"^5.0.0"}}"#;
        let lock = r#"
# yarn lockfile v1

left-pad@^1.3.0:
  version "1.3.0"
  resolved "https://registry.yarnpkg.com/left-pad/-/left-pad-1.3.0.tgz#hash"
  integrity sha512-abc

typescript@^5.0.0:
  version "5.6.3"
  resolved "https://registry.yarnpkg.com/typescript/-/typescript-5.6.3.tgz#hash"
  integrity sha512-def
"#;
        let graph = parse_yarn(manifest, lock, Path::new("package.json"), sample()).expect("yarn");
        assert!(graph.edges.iter().any(|edge| {
            edge.to == "yarn:left-pad@1.3.0" && edge.kind == DependencyKind::Normal
        }));
        assert!(graph.edges.iter().any(|edge| {
            edge.to == "yarn:typescript@5.6.3" && edge.kind == DependencyKind::Development
        }));
    }

    #[test]
    fn resolves_berry_lockfile() {
        let manifest = r#"{"name":"demo","version":"0.1.0","license":"MIT"}"#;
        let lock = r#"
__metadata:
  version: 8

"demo@workspace:.":
  version: 0.0.0-use.local
  resolution: "demo@workspace:."
  dependencies:
    left-pad: "npm:^1.3.0"
  languageName: unknown
  linkType: soft

"left-pad@npm:^1.3.0":
  version: 1.3.0
  resolution: "left-pad@npm:1.3.0"
  checksum: 10/abc
  languageName: node
  linkType: hard
"#;
        let graph = parse_yarn(manifest, lock, Path::new("package.json"), sample()).expect("berry");
        assert!(
            graph
                .packages
                .iter()
                .any(|package| package.workspace_root && package.name == "demo")
        );
        assert!(
            graph
                .edges
                .iter()
                .any(|edge| edge.to == "yarn:left-pad@1.3.0")
        );
    }

    fn sample() -> LockfileEvidence {
        LockfileEvidence {
            path: RepoPath::parse("yarn.lock").expect("path"),
            sha256: "ab".repeat(32),
            byte_len: 1,
        }
    }
}
