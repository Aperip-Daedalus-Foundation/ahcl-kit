// crates/ahcl-kit-javascript/src/npm.rs - npm package-lock.json resolution.
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
use crate::model::{ParsedEdge, ParsedGraph, ParsedPackage};
use ahcl_kit_core::{DependencyKind, LockfileEvidence, RepoPath};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(crate) fn parse_npm(
    manifest_text: &str,
    lock_text: &str,
    manifest_path: &Path,
    lockfile: LockfileEvidence,
) -> Result<ParsedGraph, JavascriptError> {
    let manifest = manifest_value(manifest_text).map_err(parse_error)?;
    let lock: Value =
        serde_json::from_str(lock_text).map_err(|error| parse_error(error.to_string()))?;
    let version = lock
        .get("lockfileVersion")
        .and_then(Value::as_u64)
        .ok_or_else(|| parse_error("npm lockfileVersion must be 2 or 3".to_owned()))?;
    if !(2..=3).contains(&version) {
        return Err(parse_error(format!(
            "unsupported npm lockfileVersion {version}; require 2 or 3"
        )));
    }
    let packages = lock
        .get("packages")
        .and_then(Value::as_object)
        .ok_or_else(|| parse_error("npm lockfile is missing packages".to_owned()))?;
    let workspaces = workspace_patterns(&manifest);
    let mut nodes = BTreeMap::new();
    for (key, value) in packages {
        nodes.insert(
            key.clone(),
            npm_package(key, value, &manifest, &workspaces)?,
        );
    }
    if !nodes.contains_key("") {
        return Err(parse_error(
            "npm lockfile is missing the root package".to_owned(),
        ));
    }

    let mut edges = Vec::new();
    let keys = nodes.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        let groups = dependency_groups(&nodes[&key]);
        for (name, kind, targets) in groups {
            let Some(target) = lookup(&nodes, &key, &name) else {
                let optional = nodes[&key].optional_dependencies.contains_key(&name)
                    || nodes[&key].peer_dependencies.contains_key(&name);
                let required = nodes[&key].dependencies.contains_key(&name)
                    || (key.is_empty() && nodes[&key].dev_dependencies.contains_key(&name));
                if optional || !required {
                    continue;
                }
                return Err(parse_error(format!(
                    "npm package {key} depends on unresolved {name}"
                )));
            };
            let target = follow_link(&nodes, target);
            edges.push(ParsedEdge {
                from: nodes[&key].id.clone(),
                to: nodes[&target].id.clone(),
                kind,
                targets,
            });
        }
    }

    let packages = nodes.into_values().map(|package| ParsedPackage {
        id: package.id,
        name: package.name,
        version: package.version,
        source: package.source,
        checksum: package.checksum,
        declared_license: package.declared_license,
        workspace_root: package.workspace_root,
        manifest_path: manifest_path.to_path_buf(),
        lockfiles: vec![lockfile.clone()],
    });
    Ok(ParsedGraph {
        packages: packages.collect(),
        edges,
    })
}

struct NpmPackage {
    id: String,
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
    declared_license: Option<String>,
    workspace_root: bool,
    link_target: Option<String>,
    dependencies: BTreeMap<String, String>,
    dev_dependencies: BTreeMap<String, String>,
    optional_dependencies: BTreeMap<String, String>,
    peer_dependencies: BTreeMap<String, String>,
}

fn npm_package(
    key: &str,
    value: &Value,
    manifest: &Value,
    workspaces: &[String],
) -> Result<NpmPackage, JavascriptError> {
    let link = value.get("link").and_then(Value::as_bool).unwrap_or(false);
    let name = if key.is_empty() {
        json_string(value, "name")
            .or_else(|| json_string(manifest, "name"))
            .ok_or_else(|| parse_error("npm root package has no name".to_owned()))?
    } else {
        json_string(value, "name")
            .or_else(|| name_from_key(key))
            .ok_or_else(|| parse_error(format!("npm package path is invalid: {key}")))?
    };
    let version = json_string(value, "version")
        .or_else(|| {
            key.is_empty()
                .then(|| json_string(manifest, "version"))
                .flatten()
        })
        .unwrap_or_else(|| "0.0.0".to_owned());
    if !key.is_empty() && version == "0.0.0" && json_string(value, "version").is_none() && !link {
        return Err(parse_error(format!("npm package {key} has no version")));
    }
    let resolved = json_string(value, "resolved");
    let link_target = link.then(|| resolved.clone()).flatten();
    let source = resolved.or_else(|| link.then(|| "link".to_owned()));
    let workspace_entry = key.is_empty() || declared_workspace(key, workspaces);
    Ok(NpmPackage {
        id: if key.is_empty() {
            format!("npm:{name}@{version}")
        } else {
            format!("npm:{key}@{version}")
        },
        name,
        version,
        checksum: json_string(value, "integrity"),
        declared_license: json_string(value, "license").or_else(|| {
            key.is_empty()
                .then(|| json_string(manifest, "license"))
                .flatten()
        }),
        workspace_root: workspace_entry,
        link_target,
        source,
        dependencies: string_map(value, "dependencies"),
        dev_dependencies: string_map(value, "devDependencies"),
        optional_dependencies: string_map(value, "optionalDependencies"),
        peer_dependencies: string_map(value, "peerDependencies"),
    })
}

fn dependency_groups(package: &NpmPackage) -> Vec<(String, DependencyKind, Vec<String>)> {
    let mut names = BTreeSet::new();
    names.extend(package.dependencies.keys().cloned());
    names.extend(package.dev_dependencies.keys().cloned());
    names.extend(package.optional_dependencies.keys().cloned());
    names.extend(package.peer_dependencies.keys().cloned());
    names
        .into_iter()
        .map(|name| {
            let mut targets = Vec::new();
            let kind = if package.dependencies.contains_key(&name)
                || package.optional_dependencies.contains_key(&name)
            {
                if package.optional_dependencies.contains_key(&name) {
                    targets.push("optional".to_owned());
                }
                DependencyKind::Normal
            } else if package.peer_dependencies.contains_key(&name) {
                targets.push("peer".to_owned());
                DependencyKind::Normal
            } else {
                DependencyKind::Development
            };
            (name, kind, targets)
        })
        .collect()
}

fn follow_link(packages: &BTreeMap<String, NpmPackage>, key: String) -> String {
    packages
        .get(&key)
        .and_then(|package| package.link_target.clone())
        .filter(|target| packages.contains_key(target))
        .unwrap_or(key)
}

fn lookup(packages: &BTreeMap<String, NpmPackage>, from: &str, name: &str) -> Option<String> {
    let mut cursor = Some(from.to_owned());
    while let Some(current) = cursor {
        let candidate = if current.is_empty() {
            format!("node_modules/{name}")
        } else {
            format!("{current}/node_modules/{name}")
        };
        if packages.contains_key(&candidate) {
            return Some(candidate);
        }
        cursor = parent_key(&current);
    }
    None
}

fn parent_key(key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    if let Some(index) = key.rfind("/node_modules/") {
        return Some(key[..index].to_owned());
    }
    if key.starts_with("node_modules/") {
        return Some(String::new());
    }
    match key.rsplit_once('/') {
        Some((parent, _)) if !parent.is_empty() => Some(parent.to_owned()),
        _ => Some(String::new()),
    }
}

fn workspace_patterns(manifest: &Value) -> Vec<String> {
    match manifest.get("workspaces") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::Object(fields)) => fields
            .get("packages")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn declared_workspace(key: &str, patterns: &[String]) -> bool {
    !key.is_empty()
        && !key.contains("node_modules/")
        && patterns
            .iter()
            .any(|pattern| workspace_pattern_matches(pattern, key))
}

fn workspace_pattern_matches(pattern: &str, key: &str) -> bool {
    let pattern = workspace_segments(pattern);
    let key = workspace_segments(key);
    let mut memo = vec![None; (pattern.len() + 1) * (key.len() + 1)];
    match_workspace_segments(&pattern, &key, 0, 0, &mut memo)
}

fn workspace_segments(value: &str) -> Vec<&str> {
    let mut segments = value
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    while segments.first() == Some(&".") {
        segments.remove(0);
    }
    segments
}

fn match_workspace_segments(
    pattern: &[&str],
    value: &[&str],
    pattern_index: usize,
    value_index: usize,
    memo: &mut [Option<bool>],
) -> bool {
    let cache_index = pattern_index * (value.len() + 1) + value_index;
    if let Some(matched) = memo[cache_index] {
        return matched;
    }
    let matched = if pattern_index == pattern.len() {
        value_index == value.len()
    } else if pattern[pattern_index] == "**" {
        match_workspace_segments(pattern, value, pattern_index + 1, value_index, memo)
            || (value_index < value.len()
                && match_workspace_segments(pattern, value, pattern_index, value_index + 1, memo))
    } else {
        value_index < value.len()
            && segment_matches(pattern[pattern_index], value[value_index])
            && match_workspace_segments(pattern, value, pattern_index + 1, value_index + 1, memo)
    };
    memo[cache_index] = Some(matched);
    matched
}

fn segment_matches(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let mut pattern_index = 0;
    let mut value_index = 0;
    let mut star = None;
    let mut star_value = 0;
    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == '?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star = Some(pattern_index);
            star_value = value_index;
            pattern_index += 1;
        } else if let Some(saved) = star {
            pattern_index = saved + 1;
            star_value += 1;
            value_index = star_value;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

fn name_from_key(key: &str) -> Option<String> {
    let rest = key.rsplit_once("node_modules/")?.1;
    if rest.starts_with('@') {
        let mut parts = rest.split('/');
        let scope = parts.next()?;
        let name = parts.next()?;
        if parts.next().is_some() || scope.len() < 2 || name.is_empty() {
            return None;
        }
        return Some(format!("{scope}/{name}"));
    }
    if rest.is_empty() || rest.contains('/') {
        return None;
    }
    Some(rest.to_owned())
}

fn parse_error(message: String) -> JavascriptError {
    JavascriptError::LockfileParse {
        path: RepoPath::parse("package-lock.json").expect("static path"),
        message,
    }
}
