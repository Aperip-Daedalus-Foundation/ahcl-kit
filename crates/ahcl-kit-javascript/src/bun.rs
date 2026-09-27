// crates/ahcl-kit-javascript/src/bun.rs - bun.lock text resolution.
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
    let lock: Value = serde_json::from_str(&strip_jsonc(lock_text))
        .map_err(|error| parse_error(error.to_string()))?;
    let packages = lock
        .get("packages")
        .and_then(Value::as_object)
        .ok_or_else(|| parse_error("bun lockfile is missing packages".to_owned()))?;
    let mut nodes = BTreeMap::new();
    for (key, value) in packages {
        let package = bun_package(key, value)?;
        nodes.insert(key.clone(), package);
    }
    let workspaces = lock.get("workspaces").and_then(Value::as_object);
    let mut parsed = Vec::new();
    let mut edges = Vec::new();
    if let Some(workspaces) = workspaces {
        for (path, workspace) in workspaces {
            let name = json_string(workspace, "name")
                .or_else(|| {
                    (path.is_empty())
                        .then(|| json_string(&manifest, "name"))
                        .flatten()
                })
                .unwrap_or_else(|| {
                    if path.is_empty() {
                        "workspace".to_owned()
                    } else {
                        path.clone()
                    }
                });
            let version = json_string(workspace, "version")
                .or_else(|| {
                    path.is_empty()
                        .then(|| json_string(&manifest, "version"))
                        .flatten()
                })
                .unwrap_or_else(|| "0.0.0".to_owned());
            let id = format!("bun:workspace:{path}:{name}@{version}");
            for (key, kind, targets) in [
                ("dependencies", DependencyKind::Normal, Vec::new()),
                (
                    "optionalDependencies",
                    DependencyKind::Normal,
                    vec!["optional".to_owned()],
                ),
                ("devDependencies", DependencyKind::Development, Vec::new()),
            ] {
                for (name, _) in string_map(workspace, key) {
                    let Some(package) = nodes.get(&name) else {
                        if key == "optionalDependencies" {
                            continue;
                        }
                        return Err(parse_error(format!(
                            "bun workspace {path} depends on unresolved {name}"
                        )));
                    };
                    edges.push(ParsedEdge {
                        from: id.clone(),
                        to: package.id.clone(),
                        kind,
                        targets: targets.clone(),
                    });
                }
            }
            parsed.push(ParsedPackage {
                id,
                name,
                version,
                source: Some("workspace".to_owned()),
                checksum: None,
                declared_license: json_string(workspace, "license").or_else(|| {
                    path.is_empty()
                        .then(|| json_string(&manifest, "license"))
                        .flatten()
                }),
                workspace_root: true,
                manifest_path: manifest_path.to_path_buf(),
                lockfiles: vec![lockfile.clone()],
            });
        }
    }
    if !parsed.iter().any(|package| package.workspace_root) {
        return Err(parse_error("bun lockfile has no workspace".to_owned()));
    }
    for package in nodes.values() {
        for name in package.dependencies.keys() {
            let Some(target) = nodes.get(name) else {
                return Err(parse_error(format!(
                    "bun package {} depends on unresolved {name}",
                    package.name
                )));
            };
            edges.push(ParsedEdge {
                from: package.id.clone(),
                to: target.id.clone(),
                kind: DependencyKind::Normal,
                targets: Vec::new(),
            });
        }
        for name in package.dev_dependencies.keys() {
            let Some(target) = nodes.get(name) else {
                return Err(parse_error(format!(
                    "bun package {} depends on unresolved {name}",
                    package.name
                )));
            };
            edges.push(ParsedEdge {
                from: package.id.clone(),
                to: target.id.clone(),
                kind: DependencyKind::Development,
                targets: Vec::new(),
            });
        }
        parsed.push(ParsedPackage {
            id: package.id.clone(),
            name: package.name.clone(),
            version: package.version.clone(),
            source: package.source.clone(),
            checksum: package.checksum.clone(),
            declared_license: None,
            workspace_root: package.workspace,
            manifest_path: manifest_path.to_path_buf(),
            lockfiles: vec![lockfile.clone()],
        });
    }
    Ok(ParsedGraph {
        packages: parsed,
        edges,
    })
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
    let chars = text.chars().collect::<Vec<_>>();
    let mut stripped = String::with_capacity(text.len());
    let mut index = 0;
    let mut string = false;
    let mut escaped = false;
    while index < chars.len() {
        let character = chars[index];
        if string {
            stripped.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                string = false;
            }
            index += 1;
            continue;
        }
        if character == '"' {
            string = true;
            stripped.push(character);
            index += 1;
            continue;
        }
        if character == '/' && chars.get(index + 1) == Some(&'/') {
            index += 2;
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if character == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            while index + 1 < chars.len() && !(chars[index] == '*' && chars[index + 1] == '/') {
                index += 1;
            }
            index = (index + 2).min(chars.len());
            continue;
        }
        stripped.push(character);
        index += 1;
    }
    strip_trailing_commas(&stripped)
}

fn strip_trailing_commas(text: &str) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    let mut stripped = String::with_capacity(text.len());
    let mut index = 0;
    let mut string = false;
    let mut escaped = false;
    while index < chars.len() {
        let character = chars[index];
        if string {
            stripped.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                string = false;
            }
            index += 1;
            continue;
        }
        if character == '"' {
            string = true;
            stripped.push(character);
            index += 1;
            continue;
        }
        if character == ',' {
            let mut look = index + 1;
            while look < chars.len() && chars[look].is_whitespace() {
                look += 1;
            }
            if matches!(chars.get(look), Some('}' | ']')) {
                index += 1;
                continue;
            }
        }
        stripped.push(character);
        index += 1;
    }
    stripped
}

fn parse_error(message: String) -> JavascriptError {
    JavascriptError::LockfileParse {
        path: RepoPath::parse("bun.lock").expect("static path"),
        message,
    }
}
