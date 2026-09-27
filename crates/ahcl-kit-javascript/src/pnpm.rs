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
    let lockfile_version = scalar(&root, "lockfileVersion")
        .unwrap_or_default()
        .trim_matches(['\'', '"'])
        .to_owned();
    if !lockfile_version.starts_with('6') && !lockfile_version.starts_with('9') {
        return Err(parse_error(format!(
            "unsupported pnpm lockfileVersion {lockfile_version}; require 6 or 9"
        )));
    }
    let packages = map(&root, "packages").cloned().unwrap_or_default();
    let snapshots = map(&root, "snapshots").cloned().unwrap_or_default();
    let version_nine = lockfile_version.starts_with('9');
    let importers = if let Some(importers) = map(&root, "importers") {
        importers.clone()
    } else {
        let mut synthetic = BTreeMap::new();
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
        synthetic.insert(".".to_owned(), Yaml::Map(importer));
        synthetic
    };

    let root_name = json_string(&manifest, "name").unwrap_or_else(|| "workspace".to_owned());
    let root_version = json_string(&manifest, "version").unwrap_or_else(|| "0.0.0".to_owned());
    let root_license = json_string(&manifest, "license");
    let mut parsed = Vec::new();
    let mut edges = Vec::new();
    let mut importer_ids = BTreeMap::new();

    for (importer_path, importer) in &importers {
        let Yaml::Map(_) = importer else {
            return Err(parse_error(format!(
                "pnpm importer {importer_path} is invalid"
            )));
        };
        let (name, version, license) = if importer_path == "." {
            (
                root_name.clone(),
                root_version.clone(),
                root_license.clone(),
            )
        } else {
            (importer_path.clone(), "0.0.0".to_owned(), None)
        };
        let id = format!("pnpm:importer:{importer_path}:{name}@{version}");
        importer_ids.insert(importer_path.clone(), id.clone());
        parsed.push(ParsedPackage {
            id,
            name,
            version,
            source: Some("workspace".to_owned()),
            checksum: None,
            declared_license: license,
            workspace_root: true,
            manifest_path: manifest_path.to_path_buf(),
            lockfiles: vec![lockfile.clone()],
        });
    }

    for (key, value) in &packages {
        let Yaml::Map(fields) = value else {
            continue;
        };
        let (name, version) = package_identity(key)
            .ok_or_else(|| parse_error(format!("pnpm package key is invalid: {key}")))?;
        let resolution = map_from(fields, "resolution");
        let integrity =
            resolution.and_then(|resolution| scalar(resolution, "integrity").map(str::to_owned));
        let tarball =
            resolution.and_then(|resolution| scalar(resolution, "tarball").map(str::to_owned));
        let id = format!("pnpm:{}", strip_peer(key));
        if !version_nine {
            add_dependency_edges(&id, fields, &packages, &importer_ids, &mut edges)?;
        }
        parsed.push(ParsedPackage {
            id,
            name,
            version,
            source: tarball.or_else(|| Some("registry".to_owned())),
            checksum: integrity,
            declared_license: None,
            workspace_root: false,
            manifest_path: manifest_path.to_path_buf(),
            lockfiles: vec![lockfile.clone()],
        });
    }

    for (importer_path, importer) in &importers {
        let Yaml::Map(fields) = importer else {
            continue;
        };
        let Some(id) = importer_ids.get(importer_path) else {
            continue;
        };
        add_importer_edges(id, fields, &packages, &importer_ids, &mut edges)?;
    }
    if version_nine {
        for (key, value) in &snapshots {
            let Yaml::Map(fields) = value else {
                continue;
            };
            let id = format!("pnpm:{}", strip_peer(key));
            add_dependency_edges(&id, fields, &packages, &importer_ids, &mut edges)?;
        }
    }

    Ok(ParsedGraph {
        packages: parsed,
        edges,
    })
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
        let Some(Yaml::Map(dependencies)) = fields.get(key) else {
            continue;
        };
        for (name, value) in dependencies {
            let version = dependency_version(value)
                .ok_or_else(|| parse_error(format!("pnpm dependency {name} has no version")))?;
            let Some(to) = resolve_pnpm_target(packages, importers, name, version) else {
                if version.starts_with("link:") {
                    continue;
                }
                return Err(parse_error(format!(
                    "pnpm dependency {name}@{version} was not found"
                )));
            };
            edges.push(ParsedEdge {
                from: from.to_owned(),
                to,
                kind,
                targets: target
                    .map(|value| vec![value.to_owned()])
                    .unwrap_or_default(),
            });
        }
    }
    Ok(())
}

fn add_dependency_edges(
    from: &str,
    fields: &BTreeMap<String, Yaml>,
    packages: &BTreeMap<String, Yaml>,
    importers: &BTreeMap<String, String>,
    edges: &mut Vec<ParsedEdge>,
) -> Result<(), JavascriptError> {
    for key in ["dependencies", "optionalDependencies"] {
        let Some(Yaml::Map(dependencies)) = fields.get(key) else {
            continue;
        };
        for (name, value) in dependencies {
            let Some(version) = dependency_version(value) else {
                continue;
            };
            let Some(to) = resolve_pnpm_target(packages, importers, name, version) else {
                continue;
            };
            let mut targets = Vec::new();
            if key == "optionalDependencies" {
                targets.push("optional".to_owned());
            }
            edges.push(ParsedEdge {
                from: from.to_owned(),
                to,
                kind: DependencyKind::Normal,
                targets,
            });
        }
    }
    Ok(())
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
        let line = lines[*index];
        if line.contains('\t') {
            return Err("pnpm lockfile cannot contain tabs".to_owned());
        }
        let current = line.len() - line.trim_start_matches(' ').len();
        if current < indent {
            break;
        }
        if current != indent {
            return Err(format!("unexpected indentation in pnpm lockfile: {line}"));
        }
        let trimmed = line.trim_start_matches(' ');
        if trimmed.starts_with("- ") {
            return Err("pnpm lockfile lists are not supported".to_owned());
        }
        let (key, value) = split_entry(trimmed)?;
        *index += 1;
        let parsed = if let Some(value) = value {
            parse_scalar_or_inline(value)?
        } else if *index < lines.len() {
            let next = lines[*index];
            let next_indent = next.len() - next.trim_start_matches(' ').len();
            if next_indent > indent {
                Yaml::Map(parse_block(lines, index, next_indent)?)
            } else {
                Yaml::Map(BTreeMap::new())
            }
        } else {
            Yaml::Map(BTreeMap::new())
        };
        map.insert(key, parsed);
    }
    Ok(map)
}

fn split_entry(line: &str) -> Result<(String, Option<&str>), String> {
    let mut quoted = None;
    for (index, character) in line.char_indices() {
        match (quoted, character) {
            (None, '\'' | '"') => quoted = Some(character),
            (Some(quote), character) if character == quote => quoted = None,
            (None, ':') => {
                let key = unquote(line[..index].trim())?;
                let value = line[index + 1..].trim();
                return Ok((key, if value.is_empty() { None } else { Some(value) }));
            }
            _ => {}
        }
    }
    Err(format!("pnpm lockfile entry has no key: {line}"))
}

fn parse_scalar_or_inline(value: &str) -> Result<Yaml, String> {
    let value = value.trim();
    if let Some(inner) = value
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
    {
        let mut map = BTreeMap::new();
        if !inner.trim().is_empty() {
            for part in split_inline(inner)? {
                let (key, raw) = split_entry(part.trim())?;
                let Some(raw) = raw else {
                    return Err(format!("inline pnpm value is incomplete: {part}"));
                };
                map.insert(key, Yaml::Scalar(unquote(raw)?));
            }
        }
        return Ok(Yaml::Map(map));
    }
    Ok(Yaml::Scalar(unquote(value)?))
}

fn split_inline(value: &str) -> Result<Vec<&str>, String> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = None;
    let mut depth = 0;
    for (index, character) in value.char_indices() {
        match (quoted, character) {
            (None, '\'' | '"') => quoted = Some(character),
            (Some(quote), character) if character == quote => quoted = None,
            (None, '{') => depth += 1,
            (None, '}') => depth -= 1,
            (None, ',') if depth == 0 => {
                parts.push(value[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    let tail = value[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    Ok(parts)
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
