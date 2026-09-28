// crates/ahcl-kit-javascript/src/model.rs - Normalized JavaScript dependency graphs.
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

use crate::JavascriptError;
use crate::adapter::JavascriptResolveRequest;
use ahcl_kit_config::PackageRuleClassification;
pub(crate) use ahcl_kit_core::sha256_hex;
use ahcl_kit_core::{
    DependencyEdge, DependencyKind, LockfileEvidence, ResolvedGraph, ResolvedPackage,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;

pub(crate) const MAX_LOCKFILE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParsedPackage {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) source: Option<String>,
    pub(crate) checksum: Option<String>,
    pub(crate) declared_license: Option<String>,
    pub(crate) workspace_root: bool,
    pub(crate) manifest_path: PathBuf,
    pub(crate) lockfiles: Vec<LockfileEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParsedEdge {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) kind: DependencyKind,
    pub(crate) targets: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ParsedGraph {
    pub(crate) packages: Vec<ParsedPackage>,
    pub(crate) edges: Vec<ParsedEdge>,
}

pub(crate) fn split_package_ident(ident: &str) -> Option<(&str, &str)> {
    let (name, version) = if let Some(rest) = ident.strip_prefix('@') {
        let (scoped, version) = rest.split_once('@')?;
        (ident.get(..scoped.len() + 1)?, version)
    } else {
        ident.split_once('@')?
    };
    if name.is_empty() || version.is_empty() {
        return None;
    }
    Some((name, version))
}

pub(crate) fn finish(
    request: &JavascriptResolveRequest,
    parsed: Vec<ParsedGraph>,
) -> Result<ResolvedGraph, JavascriptError> {
    let mut merged = merge_parsed(parsed)?;
    let selected = select_roots(request, &merged.packages, &merged.roots)?;
    let reachable = reachable_packages(&selected, &merged.edges);
    mark_direct_edges(&mut merged.edges, &selected);
    let classifications = classify_reachable(request, &merged.packages, &reachable);
    Ok(ResolvedGraph {
        packages: resolved_packages(&merged.packages, &reachable, &classifications),
        edges: resolved_edges(merged.edges, &reachable, &classifications),
    })
}

struct Merged {
    packages: BTreeMap<String, ParsedPackage>,
    edges: BTreeMap<EdgeKey, bool>,
    roots: BTreeSet<String>,
}

fn merge_parsed(parsed: Vec<ParsedGraph>) -> Result<Merged, JavascriptError> {
    let mut merged = Merged {
        packages: BTreeMap::new(),
        edges: BTreeMap::new(),
        roots: BTreeSet::new(),
    };
    for graph in parsed {
        merge_graph(&mut merged, graph)?;
    }
    merged.edges.retain(|key, _| {
        merged.packages.contains_key(&key.from) && merged.packages.contains_key(&key.to)
    });
    Ok(merged)
}

fn merge_graph(merged: &mut Merged, graph: ParsedGraph) -> Result<(), JavascriptError> {
    for package in graph.packages {
        merge_package(merged, package)?;
    }
    for edge in graph.edges {
        insert_edge(&mut merged.edges, edge);
    }
    Ok(())
}

fn merge_package(merged: &mut Merged, package: ParsedPackage) -> Result<(), JavascriptError> {
    if package.workspace_root {
        merged.roots.insert(package.id.clone());
    }
    if let Some(existing) = merged.packages.get_mut(&package.id) {
        return merge_existing(existing, package, &mut merged.roots);
    }
    merged.packages.insert(package.id.clone(), package);
    Ok(())
}

fn merge_existing(
    existing: &mut ParsedPackage,
    package: ParsedPackage,
    roots: &mut BTreeSet<String>,
) -> Result<(), JavascriptError> {
    if package_identity_conflicts(existing, &package) {
        return Err(JavascriptError::DuplicatePackage {
            package_id: package.id,
        });
    }
    merge_lockfiles(existing, package.lockfiles);
    if package.workspace_root {
        existing.workspace_root = true;
        roots.insert(package.id);
    }
    Ok(())
}

fn package_identity_conflicts(existing: &ParsedPackage, package: &ParsedPackage) -> bool {
    existing.name != package.name
        || existing.version != package.version
        || existing.source != package.source
        || existing.checksum != package.checksum
}

fn merge_lockfiles(existing: &mut ParsedPackage, lockfiles: Vec<LockfileEvidence>) {
    for lockfile in lockfiles {
        if lockfile_absent(existing, &lockfile) {
            existing.lockfiles.push(lockfile);
        }
    }
}

fn lockfile_absent(existing: &ParsedPackage, lockfile: &LockfileEvidence) -> bool {
    !existing
        .lockfiles
        .iter()
        .any(|current| current.path == lockfile.path)
}

fn insert_edge(edges: &mut BTreeMap<EdgeKey, bool>, edge: ParsedEdge) {
    let mut targets = edge.targets;
    targets.sort();
    targets.dedup();
    let key = EdgeKey {
        from: edge.from,
        to: edge.to,
        kind: kind_rank(edge.kind),
        targets,
    };
    edges.entry(key).or_insert(false);
}

fn reachable_packages(
    selected: &BTreeSet<String>,
    edges: &BTreeMap<EdgeKey, bool>,
) -> BTreeSet<String> {
    let mut reachable = BTreeSet::new();
    let mut queue: VecDeque<_> = selected.iter().cloned().collect();
    let adjacency = adjacency(edges);
    while let Some(package_id) = queue.pop_front() {
        push_reachable(&mut reachable, &mut queue, &adjacency, package_id);
    }
    reachable
}

fn push_reachable(
    reachable: &mut BTreeSet<String>,
    queue: &mut VecDeque<String>,
    adjacency: &BTreeMap<String, BTreeSet<String>>,
    package_id: String,
) {
    if !reachable.insert(package_id.clone()) {
        return;
    }
    if let Some(children) = adjacency.get(&package_id) {
        queue.extend(children.iter().cloned());
    }
}

fn mark_direct_edges(edges: &mut BTreeMap<EdgeKey, bool>, selected: &BTreeSet<String>) {
    for (key, direct) in edges {
        if selected.contains(&key.from) {
            *direct = true;
        }
    }
}

fn classify_reachable(
    request: &JavascriptResolveRequest,
    packages: &BTreeMap<String, ParsedPackage>,
    reachable: &BTreeSet<String>,
) -> BTreeMap<String, PackageRuleClassification> {
    let mut classifications = BTreeMap::new();
    for package_id in reachable {
        if let Some(classification) = classify_package(request, packages, package_id) {
            classifications.insert(package_id.clone(), classification);
        }
    }
    classifications
}

fn classify_package(
    request: &JavascriptResolveRequest,
    packages: &BTreeMap<String, ParsedPackage>,
    package_id: &str,
) -> Option<PackageRuleClassification> {
    let package = packages.get(package_id)?;
    if package.workspace_root {
        return Some(PackageRuleClassification::FirstParty);
    }
    let identity = format!("{}@{}", package.name, package.version);
    let source = package.source.as_deref().unwrap_or("");
    Some(request.classify(&identity, source))
}

fn resolved_packages(
    packages: &BTreeMap<String, ParsedPackage>,
    reachable: &BTreeSet<String>,
    classifications: &BTreeMap<String, PackageRuleClassification>,
) -> Vec<ResolvedPackage> {
    let mut resolved = Vec::new();
    for package_id in reachable {
        if let Some(package) = included_package(packages, classifications, package_id) {
            resolved.push(to_resolved(package, classifications, package_id));
        }
    }
    resolved.sort_by(|left, right| left.id.cmp(&right.id));
    resolved
}

fn included_package<'a>(
    packages: &'a BTreeMap<String, ParsedPackage>,
    classifications: &BTreeMap<String, PackageRuleClassification>,
    package_id: &str,
) -> Option<&'a ParsedPackage> {
    if classifications.get(package_id) == Some(&PackageRuleClassification::Exclude) {
        return None;
    }
    packages.get(package_id)
}

fn to_resolved(
    package: &ParsedPackage,
    classifications: &BTreeMap<String, PackageRuleClassification>,
    package_id: &str,
) -> ResolvedPackage {
    ResolvedPackage {
        id: package.id.clone(),
        name: package.name.clone(),
        version: package.version.clone(),
        source: package.source.clone(),
        checksum: package.checksum.clone(),
        manifest_path: package.manifest_path.clone(),
        repository: None,
        homepage: None,
        authors: Vec::new(),
        declared_license: package.declared_license.clone(),
        first_party: resolved_first_party(package, classifications, package_id),
        contributing_lockfiles: package.lockfiles.clone(),
        license_artifacts: Vec::new(),
    }
}

fn resolved_first_party(
    package: &ParsedPackage,
    classifications: &BTreeMap<String, PackageRuleClassification>,
    package_id: &str,
) -> bool {
    package.workspace_root
        || classifications.get(package_id) == Some(&PackageRuleClassification::FirstParty)
}

fn resolved_edges(
    edges: BTreeMap<EdgeKey, bool>,
    reachable: &BTreeSet<String>,
    classifications: &BTreeMap<String, PackageRuleClassification>,
) -> Vec<DependencyEdge> {
    let mut resolved_edges = Vec::new();
    for (key, direct) in edges {
        if edge_included(&key, reachable, classifications) {
            resolved_edges.push(DependencyEdge {
                from_package_id: key.from,
                to_package_id: key.to,
                kind: kind_from_rank(key.kind),
                target_conditions: key.targets,
                direct,
            });
        }
    }
    resolved_edges.sort_by(|left, right| {
        left.from_package_id
            .cmp(&right.from_package_id)
            .then_with(|| left.to_package_id.cmp(&right.to_package_id))
            .then_with(|| kind_rank(left.kind).cmp(&kind_rank(right.kind)))
            .then_with(|| left.target_conditions.cmp(&right.target_conditions))
            .then_with(|| left.direct.cmp(&right.direct))
    });
    resolved_edges
}

fn edge_included(
    key: &EdgeKey,
    reachable: &BTreeSet<String>,
    classifications: &BTreeMap<String, PackageRuleClassification>,
) -> bool {
    !excluded(classifications, &key.to)
        && !excluded(classifications, &key.from)
        && reachable.contains(&key.from)
        && reachable.contains(&key.to)
}

fn excluded(
    classifications: &BTreeMap<String, PackageRuleClassification>,
    package_id: &str,
) -> bool {
    classifications.get(package_id) == Some(&PackageRuleClassification::Exclude)
}

fn select_roots(
    request: &JavascriptResolveRequest,
    packages: &BTreeMap<String, ParsedPackage>,
    roots: &BTreeSet<String>,
) -> Result<BTreeSet<String>, JavascriptError> {
    if request.packages().is_empty() {
        return Ok(roots.clone());
    }
    let mut selected = BTreeSet::new();
    for selection in request.packages() {
        let matches = roots
            .iter()
            .filter(|package_id| {
                packages
                    .get(package_id.as_str())
                    .is_some_and(|package| package.name == *selection || package.id == *selection)
            })
            .cloned()
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            return Err(JavascriptError::PackageSelection {
                package: selection.clone(),
            });
        }
        let Some(package_id) = matches.into_iter().next() else {
            return Err(JavascriptError::PackageSelection {
                package: selection.clone(),
            });
        };
        selected.insert(package_id);
    }
    Ok(selected)
}

fn adjacency(edges: &BTreeMap<EdgeKey, bool>) -> BTreeMap<String, BTreeSet<String>> {
    let mut adjacency = BTreeMap::<String, BTreeSet<String>>::new();
    for key in edges.keys() {
        adjacency
            .entry(key.from.clone())
            .or_default()
            .insert(key.to.clone());
    }
    adjacency
}

const fn kind_rank(kind: DependencyKind) -> u8 {
    match kind {
        DependencyKind::Normal => 0,
        DependencyKind::Build => 1,
        DependencyKind::Development => 2,
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct EdgeKey {
    from: String,
    to: String,
    kind: u8,
    targets: Vec<String>,
}

fn kind_from_rank(kind: u8) -> DependencyKind {
    match kind {
        1 => DependencyKind::Build,
        2 => DependencyKind::Development,
        _ => DependencyKind::Normal,
    }
}
