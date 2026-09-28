// crates/ahcl-kit-javascript/src/adapter.rs - JavaScript ecosystem adapter.
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

use crate::bun::parse_bun;
use crate::error::JavascriptError;
use crate::model::{self, MAX_LOCKFILE_BYTES, ParsedGraph};
use crate::npm::parse_npm;
use crate::pnpm::parse_pnpm;
use crate::settings::{JavascriptBinding, JavascriptSettings, JsPackageManager};
use crate::yarn::parse_yarn;
use ahcl_kit_config::{EffectiveConfig, PackageRuleClassification};
use ahcl_kit_core::{
    AdapterRequest, EcosystemAdapter, LockfileEvidence, ProjectRoot, RepoPath, ResolvedGraph,
};
use ahcl_kit_fs as platform_fs;

pub struct JavascriptResolveRequest {
    project_root: ProjectRoot,
    settings: JavascriptSettings,
}

impl JavascriptResolveRequest {
    pub fn from_config(project_root: ProjectRoot, config: &EffectiveConfig) -> Self {
        Self {
            project_root,
            settings: JavascriptBinding::from_config(config)
                .map(|binding| binding.settings().clone())
                .expect("the JavaScript contributor is loaded before JavaScript resolution"),
        }
    }

    pub fn from_adapter_request(request: &AdapterRequest) -> Self {
        Self {
            project_root: request.project_root().clone(),
            settings: JavascriptSettings::from_manifests(request.manifest_paths()),
        }
    }

    pub(crate) fn project_root(&self) -> &ProjectRoot {
        &self.project_root
    }

    pub(crate) fn manifests(&self) -> &[RepoPath] {
        self.settings.manifests()
    }

    pub(crate) fn packages(&self) -> &[String] {
        self.settings.packages()
    }

    pub(crate) fn classify(&self, package: &str, source: &str) -> PackageRuleClassification {
        self.settings.classify(package, source)
    }
}

pub struct JavascriptAdapter;

impl JavascriptAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn resolve_request(
        &self,
        request: &JavascriptResolveRequest,
    ) -> Result<ResolvedGraph, JavascriptError> {
        let mut parsed = Vec::new();
        for manifest in request.manifests() {
            let manifest_path = read_manifest(request, manifest)?;
            let managers = selected_managers(request, manifest)?;
            for manager in managers {
                parsed.push(parse_one(request, manifest, &manifest_path, manager)?);
            }
        }
        model::finish(request, parsed)
    }
}

impl Default for JavascriptAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl EcosystemAdapter for JavascriptAdapter {
    fn ecosystem(&self) -> &'static str {
        "javascript"
    }

    fn resolve(
        &self,
        request: &AdapterRequest,
    ) -> Result<ResolvedGraph, Box<dyn std::error::Error + Send + Sync>> {
        self.resolve_request(&JavascriptResolveRequest::from_adapter_request(request))
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
    }
}

fn read_manifest(
    request: &JavascriptResolveRequest,
    manifest: &RepoPath,
) -> Result<String, JavascriptError> {
    let bytes = read_bounded(request, manifest).map_err(|error| match error {
        platform_fs::PackageFsError::FileTooLarge(_) => JavascriptError::LockfileTooLarge {
            path: manifest.clone(),
        },
        _ => JavascriptError::ManifestInvalid {
            manifest: manifest.clone(),
        },
    })?;
    String::from_utf8(bytes).map_err(|_| JavascriptError::LockfileParse {
        path: manifest.clone(),
        message: "manifest is not UTF-8".to_owned(),
    })
}

fn parse_one(
    request: &JavascriptResolveRequest,
    manifest: &RepoPath,
    manifest_text: &str,
    manager: JsPackageManager,
) -> Result<ParsedGraph, JavascriptError> {
    let file_name = lockfile_name(manager);
    reject_binary_bun(request, manifest, manager)?;
    let lock_path = sibling(manifest, file_name)?;
    let bytes = read_lockfile(request, &lock_path)?;
    let text = lockfile_text(&lock_path, &bytes)?;
    let evidence = lock_evidence(&lock_path, &bytes);
    let manifest_path = request.project_root().resolve(manifest);
    parse_manager_lock(manager, manifest_text, &text, &manifest_path, evidence)
        .map_err(|error| relocate_parse_error(error, lock_path))
}

fn reject_binary_bun(
    request: &JavascriptResolveRequest,
    manifest: &RepoPath,
    manager: JsPackageManager,
) -> Result<(), JavascriptError> {
    if manager != JsPackageManager::Bun || lockfile_exists(request, manifest, "bun.lock") {
        return Ok(());
    }
    let binary = sibling(manifest, "bun.lockb")?;
    if lockfile_exists(request, manifest, "bun.lockb") {
        return Err(JavascriptError::BinaryBunLockfile { path: binary });
    }
    Ok(())
}

fn lockfile_text(lock_path: &RepoPath, bytes: &[u8]) -> Result<String, JavascriptError> {
    String::from_utf8(bytes.to_vec()).map_err(|_| JavascriptError::LockfileParse {
        path: lock_path.clone(),
        message: "lockfile is not UTF-8".to_owned(),
    })
}

fn lock_evidence(lock_path: &RepoPath, bytes: &[u8]) -> LockfileEvidence {
    LockfileEvidence {
        path: lock_path.clone(),
        sha256: model::sha256_hex(bytes),
        byte_len: bytes.len() as u64,
    }
}

fn parse_manager_lock(
    manager: JsPackageManager,
    manifest_text: &str,
    text: &str,
    manifest_path: &std::path::Path,
    evidence: LockfileEvidence,
) -> Result<ParsedGraph, JavascriptError> {
    match manager {
        JsPackageManager::Npm => parse_npm(manifest_text, text, manifest_path, evidence),
        JsPackageManager::Pnpm => parse_pnpm(manifest_text, text, manifest_path, evidence),
        JsPackageManager::Yarn => parse_yarn(manifest_text, text, manifest_path, evidence),
        JsPackageManager::Bun => parse_bun(manifest_text, text, manifest_path, evidence),
    }
}

fn relocate_parse_error(error: JavascriptError, lock_path: RepoPath) -> JavascriptError {
    match error {
        JavascriptError::LockfileParse { message, .. } => JavascriptError::LockfileParse {
            path: lock_path,
            message,
        },
        other => other,
    }
}

fn selected_managers(
    request: &JavascriptResolveRequest,
    manifest: &RepoPath,
) -> Result<Vec<JsPackageManager>, JavascriptError> {
    let configured = request.settings.managers();
    if !configured.is_empty() {
        return configured_managers(request, manifest, configured);
    }
    discovered_managers(request, manifest)
}

fn configured_managers(
    request: &JavascriptResolveRequest,
    manifest: &RepoPath,
    configured: &[JsPackageManager],
) -> Result<Vec<JsPackageManager>, JavascriptError> {
    for manager in configured {
        require_configured_lockfile(request, manifest, *manager)?;
    }
    Ok(configured.to_vec())
}

fn require_configured_lockfile(
    request: &JavascriptResolveRequest,
    manifest: &RepoPath,
    manager: JsPackageManager,
) -> Result<(), JavascriptError> {
    if lockfile_exists(request, manifest, lockfile_name(manager)) {
        return Ok(());
    }
    if manager == JsPackageManager::Bun && lockfile_exists(request, manifest, "bun.lockb") {
        return Err(JavascriptError::BinaryBunLockfile {
            path: sibling(manifest, "bun.lockb")?,
        });
    }
    Err(JavascriptError::LockfileMissing {
        manifest: manifest.clone(),
    })
}

fn discovered_managers(
    request: &JavascriptResolveRequest,
    manifest: &RepoPath,
) -> Result<Vec<JsPackageManager>, JavascriptError> {
    let found = present_managers(request, manifest);
    if found.is_empty() {
        return missing_discovered_lockfile(request, manifest);
    }
    if found.len() > 1 {
        return Err(JavascriptError::LockfileAmbiguous {
            manifest: manifest.clone(),
            found: found
                .iter()
                .map(|manager| manager.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        });
    }
    Ok(found)
}

fn present_managers(
    request: &JavascriptResolveRequest,
    manifest: &RepoPath,
) -> Vec<JsPackageManager> {
    let mut found = Vec::new();
    for manager in [
        JsPackageManager::Npm,
        JsPackageManager::Pnpm,
        JsPackageManager::Yarn,
        JsPackageManager::Bun,
    ] {
        if lockfile_exists(request, manifest, lockfile_name(manager)) {
            found.push(manager);
        }
    }
    found
}

fn missing_discovered_lockfile(
    request: &JavascriptResolveRequest,
    manifest: &RepoPath,
) -> Result<Vec<JsPackageManager>, JavascriptError> {
    if lockfile_exists(request, manifest, "bun.lockb") {
        return Err(JavascriptError::BinaryBunLockfile {
            path: sibling(manifest, "bun.lockb")?,
        });
    }
    Err(JavascriptError::LockfileMissing {
        manifest: manifest.clone(),
    })
}

fn lockfile_name(manager: JsPackageManager) -> &'static str {
    match manager {
        JsPackageManager::Npm => "package-lock.json",
        JsPackageManager::Pnpm => "pnpm-lock.yaml",
        JsPackageManager::Yarn => "yarn.lock",
        JsPackageManager::Bun => "bun.lock",
    }
}

fn lockfile_exists(
    request: &JavascriptResolveRequest,
    manifest: &RepoPath,
    file_name: &str,
) -> bool {
    sibling(manifest, file_name).ok().is_some_and(|path| {
        platform_fs::validate_regular_file(request.project_root().as_path(), path.as_path()).is_ok()
    })
}

fn read_lockfile(
    request: &JavascriptResolveRequest,
    lock_path: &RepoPath,
) -> Result<Vec<u8>, JavascriptError> {
    read_bounded(request, lock_path).map_err(|error| match error {
        platform_fs::PackageFsError::FileTooLarge(_) => JavascriptError::LockfileTooLarge {
            path: lock_path.clone(),
        },
        _ => JavascriptError::LockfileRead {
            path: request.project_root().resolve(lock_path),
        },
    })
}

fn read_bounded(
    request: &JavascriptResolveRequest,
    relative: &RepoPath,
) -> Result<Vec<u8>, platform_fs::PackageFsError> {
    let directory = platform_fs::PackageDirectory::open(request.project_root().as_path())?;
    directory
        .read_bounded_file(relative.as_path(), MAX_LOCKFILE_BYTES)?
        .ok_or(platform_fs::PackageFsError::InvalidPath)
}

pub(crate) fn sibling(manifest: &RepoPath, file_name: &str) -> Result<RepoPath, JavascriptError> {
    let joined = match manifest.as_str().rsplit_once('/') {
        Some((directory, _)) => format!("{directory}/{file_name}"),
        None => file_name.to_owned(),
    };
    RepoPath::parse(joined).map_err(|_| JavascriptError::PathInvalid {
        path: file_name.to_owned(),
    })
}

pub(crate) fn manifest_value(text: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(text).map_err(|error| error.to_string())
}

pub(crate) fn json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(|value| match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Object(fields) => fields
            .get("type")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        _ => None,
    })
}

pub(crate) fn string_map(value: &serde_json::Value, key: &str) -> BTreeStringMap {
    let Some(object) = value.get(key).and_then(serde_json::Value::as_object) else {
        return BTreeStringMap::new();
    };
    object
        .iter()
        .filter_map(|(name, spec)| spec.as_str().map(|spec| (name.clone(), spec.to_owned())))
        .collect()
}

pub(crate) type BTreeStringMap = std::collections::BTreeMap<String, String>;
