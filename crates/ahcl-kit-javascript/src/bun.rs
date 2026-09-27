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
    let lock: Value =
        serde_json::from_str(lock_text).map_err(|error| parse_error(error.to_string()))?;
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
                lockfile: lockfile.clone(),
            });
        }
    }
    if !parsed.iter().any(|package| package.workspace_root) {
        return Err(parse_error("bun lockfile has no workspace".to_owned()));
    }
    for package in nodes.values() {
        for name in package.dependencies.keys() {
            let Some(target) = nodes.get(name) else {
                continue;
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
                continue;
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
            lockfile: lockfile.clone(),
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

fn parse_error(message: String) -> JavascriptError {
    JavascriptError::LockfileParse {
        path: RepoPath::parse("bun.lock").expect("static path"),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_text_lockfile() {
        let manifest = r#"{"name":"demo","version":"0.1.0","license":"MIT"}"#;
        let lock = r#"{
          "lockfileVersion": 1,
          "workspaces": {
            "": {
              "name": "demo",
              "dependencies": { "left-pad": "^1.3.0" },
              "devDependencies": { "typescript": "^5.0.0" }
            }
          },
          "packages": {
            "left-pad": ["left-pad@1.3.0", "https://registry.npmjs.org/left-pad/-/left-pad-1.3.0.tgz", {}, "sha512-abc"],
            "typescript": ["typescript@5.6.3", "https://registry.npmjs.org/typescript/-/typescript-5.6.3.tgz", {}, "sha512-def"]
          }
        }"#;
        let graph = parse_bun(manifest, lock, Path::new("package.json"), sample()).expect("bun");
        assert!(graph.edges.iter().any(|edge| {
            edge.to == "bun:left-pad@1.3.0" && edge.kind == DependencyKind::Normal
        }));
        assert!(graph.edges.iter().any(|edge| {
            edge.to == "bun:typescript@5.6.3" && edge.kind == DependencyKind::Development
        }));
    }

    fn sample() -> LockfileEvidence {
        LockfileEvidence {
            path: RepoPath::parse("bun.lock").expect("path"),
            sha256: "ab".repeat(32),
            byte_len: 1,
        }
    }
}
