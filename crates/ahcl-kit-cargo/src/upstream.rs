// crates/ahcl-kit-cargo/src/upstream.rs - Upstream license evidence recovery.
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

use crate::adapter::{CargoError, CargoResolveRequest};
use crate::collector::EvidenceBudget;
use ahcl_kit_config::{CargoEvidence, CargoEvidenceKind};
use ahcl_kit_core::{LicenseArtifact, RepoPath};
use cargo_metadata::Package;
use serde_json::Value as JsonValue;
use std::path::Path;
use std::time::Duration;
use url::Url;

const MAX_UPSTREAM_RESPONSE_BYTES: u64 = 2_097_152;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CargoEvidenceRequest {
    url: String,
    path: String,
    package: String,
    version: String,
    revision: String,
    max_bytes: u64,
}

impl CargoEvidenceRequest {
    fn new(url: String, path: String, package: &Package, revision: &str, max_bytes: u64) -> Self {
        Self {
            url,
            path,
            package: package.name.to_string(),
            version: package.version.to_string(),
            revision: revision.to_owned(),
            max_bytes,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn package(&self) -> &str {
        &self.package
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CargoEvidenceResponse {
    status: u16,
    body: Vec<u8>,
}

impl CargoEvidenceResponse {
    pub fn new(status: u16, body: Vec<u8>) -> Self {
        Self { status, body }
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CargoTransportError {
    RequestFailed,
    ResponseTooLarge,
}

pub trait CargoEvidenceTransport: Send + Sync {
    fn fetch(
        &self,
        request: &CargoEvidenceRequest,
    ) -> Result<CargoEvidenceResponse, CargoTransportError>;
}

pub(crate) struct UreqCargoEvidenceTransport {
    agent: ureq::Agent,
}

impl UreqCargoEvidenceTransport {
    pub(crate) fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .https_only(true)
            .proxy(None)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_global(Some(Duration::from_secs(30)))
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl CargoEvidenceTransport for UreqCargoEvidenceTransport {
    fn fetch(
        &self,
        request: &CargoEvidenceRequest,
    ) -> Result<CargoEvidenceResponse, CargoTransportError> {
        let mut response = self
            .agent
            .get(request.url())
            .call()
            .map_err(|_| CargoTransportError::RequestFailed)?;
        let body = response
            .body_mut()
            .with_config()
            .limit(request.max_bytes().saturating_add(1))
            .read_to_vec()
            .map_err(|error| match error {
                ureq::Error::BodyExceedsLimit(_) => CargoTransportError::ResponseTooLarge,
                _ => CargoTransportError::RequestFailed,
            })?;
        if body.len() as u64 > request.max_bytes() {
            return Err(CargoTransportError::ResponseTooLarge);
        }
        Ok(CargoEvidenceResponse::new(response.status().as_u16(), body))
    }
}

pub(crate) fn recover(
    package: &Package,
    request: &CargoResolveRequest,
    transport: &dyn CargoEvidenceTransport,
    budget: &mut EvidenceBudget,
    existing: &[LicenseArtifact],
) -> Result<Vec<LicenseArtifact>, CargoError> {
    let source = package
        .source
        .as_ref()
        .map_or_else(|| "path".to_owned(), ToString::to_string);
    let mapping = request.settings().and_then(|settings| {
        find_mapping(
            settings.evidence(),
            package,
            &source,
            CargoEvidenceKind::License,
        )
    });
    if mapping.is_none()
        && !package
            .repository
            .as_deref()
            .is_some_and(is_allowed_repository)
    {
        return Ok(Vec::new());
    }
    let revision = exact_revision(package, mapping)?;
    if let Some(mapping) = mapping {
        validate_mapping(package, &source, mapping, &revision)?;
    }
    let repository = package
        .repository
        .as_deref()
        .or_else(|| mapping.map(|m| m.repository()));
    let Some(repository) = repository else {
        return Ok(Vec::new());
    };
    let manifest_hint = mapping.map_or("Cargo.toml", |mapping| mapping.path().as_str());
    let manifest_path = upstream_manifest_path(package, manifest_hint)?;
    let manifest_url = immutable_url(repository, &revision, &manifest_path)?;
    let manifest_request = CargoEvidenceRequest::new(
        manifest_url,
        manifest_path.clone(),
        package,
        &revision,
        MAX_UPSTREAM_RESPONSE_BYTES,
    );
    let manifest = fetch_text(transport, &manifest_request, "manifest")?;
    let manifest_value: toml::Value = toml::from_str(&manifest).map_err(|error| {
        upstream_error(package, "manifest", format!("invalid Cargo.toml: {error}"))
    })?;
    let evidence_path = if let Some(mapping) = mapping {
        verify_manifest(
            package,
            &manifest_value,
            &manifest_path,
            mapping.path().as_str(),
            transport,
            repository,
            &revision,
        )?;
        mapping.path().as_str().to_owned()
    } else {
        let candidate = manifest_license_candidate(
            package,
            &manifest_value,
            &manifest_path,
            transport,
            repository,
            &revision,
        )?;
        let Some(candidate) = candidate else {
            return Ok(Vec::new());
        };
        candidate
    };
    let evidence_url = mapping.map_or_else(
        || immutable_url(repository, &revision, &evidence_path),
        |mapping| Ok(mapping.url().to_owned()),
    )?;
    if let Some(mapping) = mapping {
        validate_source_url(
            &evidence_url,
            mapping.repository(),
            &revision,
            mapping.path().as_str(),
        )?;
    }
    let evidence_request = CargoEvidenceRequest::new(
        evidence_url.clone(),
        evidence_path.clone(),
        package,
        &revision,
        MAX_UPSTREAM_RESPONSE_BYTES,
    );
    let response = transport
        .fetch(&evidence_request)
        .map_err(|error| upstream_error(package, &evidence_path, transport_error(error)))?;
    if response.status() != 200 {
        return Err(upstream_error(
            package,
            &evidence_path,
            format!("upstream returned HTTP {}", response.status()),
        ));
    }
    let package_relative = relative_to_manifest_dir(
        &manifest_path,
        &evidence_path,
        manifest_inherits_license(&manifest_value),
    )?;
    let relative = RepoPath::parse(&package_relative).map_err(|_| {
        upstream_error(
            package,
            &evidence_path,
            "evidence path is not repository-safe".to_owned(),
        )
    })?;
    let mut recovered = Vec::new();
    push_artifact(
        &mut recovered,
        budget,
        request.limits().max_aggregate_bytes(),
        relative,
        response.body().to_vec(),
        package,
    )?;

    let source_note = format!(
        "License files are sourced from the owning repository.\n<{}>\n",
        evidence_url
    );
    if !existing
        .iter()
        .any(|artifact| artifact.relative_path.as_str() == "AHCL-EVIDENCE-SOURCE.md")
    {
        push_artifact(
            &mut recovered,
            budget,
            request.limits().max_aggregate_bytes(),
            RepoPath::parse("AHCL-EVIDENCE-SOURCE.md").expect("static path"),
            source_note.into_bytes(),
            package,
        )?;
    }
    Ok(recovered)
}

pub(crate) fn supplement_materials(
    package: &Package,
    request: &CargoResolveRequest,
    budget: &mut EvidenceBudget,
    existing: &[LicenseArtifact],
) -> Result<Vec<LicenseArtifact>, CargoError> {
    let source = package
        .source
        .as_ref()
        .map_or_else(|| "path".to_owned(), ToString::to_string);
    let Some(mapping) = request.settings().and_then(|settings| {
        find_mapping(
            settings.evidence(),
            package,
            &source,
            CargoEvidenceKind::Materials,
        )
    }) else {
        return Ok(Vec::new());
    };
    if mapping.kind() != CargoEvidenceKind::Materials {
        return Ok(Vec::new());
    }
    let revision = exact_revision(package, Some(mapping))?;
    validate_mapping(package, &source, mapping, &revision)?;
    if existing
        .iter()
        .any(|artifact| artifact.relative_path.as_str() == "AHCL-MATERIALS.url")
    {
        return Ok(Vec::new());
    }
    let bytes = format!("[InternetShortcut]\nURL={}\n", mapping.url()).into_bytes();
    let mut result = Vec::new();
    push_artifact(
        &mut result,
        budget,
        request.limits().max_aggregate_bytes(),
        RepoPath::parse("AHCL-MATERIALS.url").expect("static path"),
        bytes,
        package,
    )?;
    Ok(result)
}

fn find_mapping<'a>(
    mappings: &'a [CargoEvidence],
    package: &Package,
    source: &str,
    kind: CargoEvidenceKind,
) -> Option<&'a CargoEvidence> {
    mappings.iter().find(|mapping| {
        mapping.package() == package.name.to_string()
            && mapping.version() == package.version.to_string()
            && mapping.source() == source
            && mapping.kind() == kind
    })
}

pub(crate) fn recover_notice(
    package: &Package,
    request: &CargoResolveRequest,
    transport: &dyn CargoEvidenceTransport,
    budget: &mut EvidenceBudget,
    existing: &[LicenseArtifact],
) -> Result<Vec<LicenseArtifact>, CargoError> {
    let source = package
        .source
        .as_ref()
        .map_or_else(|| "path".to_owned(), ToString::to_string);
    let Some(mapping) = request.settings().and_then(|settings| {
        find_mapping(
            settings.evidence(),
            package,
            &source,
            CargoEvidenceKind::Notice,
        )
    }) else {
        return Ok(Vec::new());
    };
    let revision = exact_revision(package, Some(mapping))?;
    validate_mapping(package, &source, mapping, &revision)?;
    let repository = package
        .repository
        .as_deref()
        .unwrap_or(mapping.repository());
    let manifest_path = upstream_manifest_path(package, mapping.path().as_str())?;
    let evidence_url = mapping.url().to_owned();
    validate_source_url(
        &evidence_url,
        repository,
        &revision,
        mapping.path().as_str(),
    )?;
    if existing.iter().any(|artifact| {
        artifact
            .relative_path
            .as_path()
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_uppercase().starts_with("NOTICE"))
    }) {
        return Ok(Vec::new());
    }
    let evidence_request = CargoEvidenceRequest::new(
        evidence_url.clone(),
        mapping.path().as_str().to_owned(),
        package,
        &revision,
        MAX_UPSTREAM_RESPONSE_BYTES,
    );
    let response = transport.fetch(&evidence_request).map_err(|error| {
        upstream_error(package, mapping.path().as_str(), transport_error(error))
    })?;
    if response.status() != 200 {
        return Err(upstream_error(
            package,
            mapping.path().as_str(),
            format!("upstream returned HTTP {}", response.status()),
        ));
    }
    let package_relative = relative_to_manifest_dir(&manifest_path, mapping.path().as_str(), true)?;
    let relative = RepoPath::parse(&package_relative).map_err(|_| {
        upstream_error(
            package,
            mapping.path().as_str(),
            "evidence path is not repository-safe".to_owned(),
        )
    })?;
    let mut recovered = Vec::new();
    push_artifact(
        &mut recovered,
        budget,
        request.limits().max_aggregate_bytes(),
        relative,
        response.body().to_vec(),
        package,
    )?;
    if !existing
        .iter()
        .any(|artifact| artifact.relative_path.as_str() == "AHCL-EVIDENCE-SOURCE.md")
    {
        let source_note = format!(
            "License files are sourced from the owning repository.\n<{}>\n",
            evidence_url
        );
        push_artifact(
            &mut recovered,
            budget,
            request.limits().max_aggregate_bytes(),
            RepoPath::parse("AHCL-EVIDENCE-SOURCE.md").expect("static path"),
            source_note.into_bytes(),
            package,
        )?;
    }
    Ok(recovered)
}

fn exact_revision(
    package: &Package,
    mapping: Option<&CargoEvidence>,
) -> Result<String, CargoError> {
    if let Some(source) = package.source.as_ref() {
        let source = source.to_string();
        if let Some(fragment) = source.rsplit_once('#').map(|(_, value)| value) {
            if is_revision(fragment) {
                return Ok(fragment.to_owned());
            }
            if source.starts_with("git+") {
                return Err(upstream_error(
                    package,
                    "source",
                    "Cargo git source does not contain a full commit revision".to_owned(),
                ));
            }
        }
    }
    let root = Path::new(package.manifest_path.as_std_path())
        .parent()
        .ok_or_else(|| {
            upstream_error(package, "source", "package root is unavailable".to_owned())
        })?;
    let vcs = root.join(".cargo_vcs_info.json");
    let bytes = match std::fs::read(&vcs) {
        Ok(bytes) => bytes,
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && mapping.is_some_and(|mapping| is_revision(mapping.revision())) =>
        {
            return Ok(mapping.expect("checked").revision().to_owned());
        }
        Err(error) => {
            return Err(upstream_error(
                package,
                &vcs.to_string_lossy(),
                format!("exact revision unavailable: {error}"),
            ));
        }
    };
    let value: JsonValue = serde_json::from_slice(&bytes).map_err(|error| {
        upstream_error(
            package,
            ".cargo_vcs_info.json",
            format!("invalid metadata: {error}"),
        )
    })?;
    let revision = value
        .get("git")
        .and_then(|git| git.get("sha1"))
        .and_then(JsonValue::as_str)
        .filter(|value| is_revision(value))
        .or_else(|| {
            mapping
                .filter(|mapping| is_revision(mapping.revision()))
                .map(|mapping| mapping.revision())
        })
        .ok_or_else(|| {
            upstream_error(
                package,
                ".cargo_vcs_info.json",
                "metadata has no full commit revision".to_owned(),
            )
        })?;
    Ok(revision.to_owned())
}

fn upstream_manifest_path(package: &Package, evidence_path: &str) -> Result<String, CargoError> {
    let root = Path::new(package.manifest_path.as_std_path())
        .parent()
        .ok_or_else(|| {
            upstream_error(
                package,
                evidence_path,
                "package root unavailable".to_owned(),
            )
        })?;
    let vcs = root.join(".cargo_vcs_info.json");
    match std::fs::read(&vcs) {
        Ok(bytes) => {
            let value = serde_json::from_slice::<JsonValue>(&bytes).map_err(|error| {
                upstream_error(
                    package,
                    ".cargo_vcs_info.json",
                    format!("invalid metadata: {error}"),
                )
            })?;
            if let Some(path) = value.get("path_in_vcs").and_then(JsonValue::as_str) {
                let path = path.trim_matches('/');
                if path.is_empty() || path.split('/').any(|part| part == ".." || part.is_empty()) {
                    return Err(upstream_error(
                        package,
                        ".cargo_vcs_info.json",
                        "metadata path_in_vcs is not repository-safe".to_owned(),
                    ));
                }
                return Ok(format!("{path}/Cargo.toml"));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(upstream_error(
                package,
                &vcs.to_string_lossy(),
                format!("cannot read metadata: {error}"),
            ));
        }
    }
    let path = Path::new(evidence_path);
    let parent = path.parent().and_then(Path::to_str).unwrap_or_default();
    if parent.is_empty() {
        Ok("Cargo.toml".to_owned())
    } else {
        Ok(format!("{parent}/Cargo.toml"))
    }
}

fn manifest_license_candidate(
    package: &Package,
    manifest: &toml::Value,
    manifest_path: &str,
    transport: &dyn CargoEvidenceTransport,
    repository: &str,
    revision: &str,
) -> Result<Option<String>, CargoError> {
    let package_table = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            upstream_error(
                package,
                manifest_path,
                "manifest has no [package]".to_owned(),
            )
        })?;
    let name = package_table.get("name").and_then(toml::Value::as_str);
    let version = package_table.get("version").and_then(toml::Value::as_str);
    if name != Some(package.name.as_str()) || version != Some(package.version.to_string().as_str())
    {
        return Err(upstream_error(
            package,
            manifest_path,
            "upstream manifest package identity does not match Cargo metadata".to_owned(),
        ));
    }
    let license = package_table.get("license");
    let license_file = package_table
        .get("license-file")
        .and_then(toml::Value::as_str);
    let inherited = license
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("workspace"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    if license.is_none() && license_file.is_none() && !inherited {
        return Err(upstream_error(
            package,
            manifest_path,
            "manifest does not establish license applicability".to_owned(),
        ));
    }
    if inherited {
        let root_request = CargoEvidenceRequest::new(
            immutable_url(repository, revision, "Cargo.toml")?,
            "Cargo.toml".to_owned(),
            package,
            revision,
            MAX_UPSTREAM_RESPONSE_BYTES,
        );
        let root_manifest = fetch_text(transport, &root_request, "workspace manifest")?;
        let root_manifest: toml::Value = toml::from_str(&root_manifest).map_err(|error| {
            upstream_error(
                package,
                "Cargo.toml",
                format!("invalid workspace Cargo.toml: {error}"),
            )
        })?;
        let workspace_license = root_manifest
            .get("workspace")
            .and_then(toml::Value::as_table)
            .and_then(|workspace| workspace.get("package"))
            .and_then(toml::Value::as_table)
            .and_then(|package| {
                package
                    .get("license")
                    .or_else(|| package.get("license-file"))
            });
        if workspace_license.is_none() {
            return Err(upstream_error(
                package,
                "Cargo.toml",
                "workspace inheritance does not establish license applicability".to_owned(),
            ));
        }
    }
    if let Some(license_file) = license_file {
        return Ok(Some(join_repo_path(manifest_path, license_file)?));
    }
    let parent = Path::new(manifest_path)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or_default();
    for name in ["LICENSE", "COPYING", "COPYRIGHT"] {
        let path = if parent.is_empty() {
            name.to_owned()
        } else {
            format!("{parent}/{name}")
        };
        let url = immutable_url(repository, revision, &path)?;
        let request = CargoEvidenceRequest::new(
            url,
            path.clone(),
            package,
            revision,
            MAX_UPSTREAM_RESPONSE_BYTES,
        );
        let response = transport
            .fetch(&request)
            .map_err(|error| upstream_error(package, &path, transport_error(error)))?;
        if response.status() == 200 {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn validate_mapping(
    package: &Package,
    source: &str,
    mapping: &CargoEvidence,
    revision: &str,
) -> Result<(), CargoError> {
    if mapping.source() != source {
        return Err(upstream_error(
            package,
            mapping.path().as_str(),
            "evidence source does not match Cargo metadata".to_owned(),
        ));
    }
    if !is_revision(revision) || mapping.revision() != revision {
        return Err(upstream_error(
            package,
            mapping.path().as_str(),
            "evidence revision does not match Cargo metadata".to_owned(),
        ));
    }
    let source_revision = package
        .source
        .as_ref()
        .and_then(|source| {
            source
                .to_string()
                .rsplit_once('#')
                .map(|(_, value)| value.to_owned())
        })
        .filter(|value| is_revision(value));
    if let Some(source_revision) = source_revision.as_deref() {
        if source_revision != revision {
            return Err(upstream_error(
                package,
                mapping.path().as_str(),
                "evidence revision does not match Cargo source metadata".to_owned(),
            ));
        }
    }
    let repository = package
        .repository
        .as_deref()
        .unwrap_or(mapping.repository());
    if normalize_repository(repository) != normalize_repository(mapping.repository()) {
        return Err(upstream_error(
            package,
            mapping.path().as_str(),
            "evidence repository does not match Cargo metadata".to_owned(),
        ));
    }
    if mapping.kind() == CargoEvidenceKind::Materials {
        validate_materials_url(
            mapping.url(),
            mapping.repository(),
            revision,
            mapping.path().as_str(),
        )?;
    } else {
        validate_source_url(
            mapping.url(),
            mapping.repository(),
            revision,
            mapping.path().as_str(),
        )?;
    }
    Ok(())
}

fn validate_source_url(
    url: &str,
    repository: &str,
    revision: &str,
    path: &str,
) -> Result<(), CargoError> {
    let Some((repository_host, repository_parts)) = repository_identity(repository) else {
        return Err(upstream_url_error(
            url,
            "repository must be an HTTPS GitHub or GitLab URL",
        ));
    };
    let Some(parsed) = immutable_source_url(url) else {
        return Err(upstream_url_error(
            url,
            "evidence URL must be an immutable HTTPS provider URL",
        ));
    };
    let segments = parsed
        .path_segments()
        .map(|segments| segments.collect::<Vec<_>>())
        .unwrap_or_default();
    let path_segments = path.split('/').collect::<Vec<_>>();
    let revision_matches = |value: &str| value.eq_ignore_ascii_case(revision);
    let path_matches = |tail: &[&str]| tail == path_segments.as_slice();
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    let matches = if repository_host == "github.com" {
        let [owner, repo] = repository_parts.as_slice() else {
            return Err(upstream_url_error(
                url,
                "repository path must be owner/repository",
            ));
        };
        (host == "raw.githubusercontent.com"
            && segments.len() >= 3
            && segments[0].eq_ignore_ascii_case(owner)
            && segments[1].eq_ignore_ascii_case(repo)
            && revision_matches(segments[2])
            && path_matches(&segments[3..]))
            || (host == "github.com"
                && segments.len() >= 5
                && segments[0].eq_ignore_ascii_case(owner)
                && segments[1].eq_ignore_ascii_case(repo)
                && matches!(segments[2], "blob" | "raw")
                && revision_matches(segments[3])
                && path_matches(&segments[4..]))
    } else {
        let repository_len = repository_parts.len();
        host == "gitlab.com"
            && repository_len >= 2
            && segments.len() >= repository_len + 3
            && segments[..repository_len]
                .iter()
                .zip(&repository_parts)
                .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
            && segments[repository_len] == "-"
            && segments[repository_len + 1] == "raw"
            && revision_matches(segments[repository_len + 2])
            && path_matches(&segments[repository_len + 3..])
    };
    if !matches {
        return Err(upstream_url_error(
            url,
            "evidence URL does not match the configured repository, revision, and path",
        ));
    }
    Ok(())
}

fn validate_materials_url(
    url: &str,
    repository: &str,
    revision: &str,
    path: &str,
) -> Result<(), CargoError> {
    let Some((repository_host, repository_parts)) = repository_identity(repository) else {
        return Err(upstream_url_error(
            url,
            "repository must be an HTTPS GitHub or GitLab URL",
        ));
    };
    let Some(parsed) = immutable_source_url(url) else {
        return Err(upstream_url_error(
            url,
            "materials URL must be an immutable HTTPS provider URL",
        ));
    };
    let segments = parsed
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let path_segments = path.split('/').collect::<Vec<_>>();
    let revision_matches = |value: &str| value.eq_ignore_ascii_case(revision);
    let path_matches = |tail: &[&str]| tail == path_segments.as_slice();
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    let matches = if repository_host == "github.com" {
        let [owner, repo] = repository_parts.as_slice() else {
            return Err(upstream_url_error(
                url,
                "repository path must be owner/repository",
            ));
        };
        (host == "github.com"
            && segments.len() >= 5
            && segments[0].eq_ignore_ascii_case(owner)
            && segments[1].eq_ignore_ascii_case(repo)
            && matches!(segments[2], "blob" | "raw" | "tree")
            && revision_matches(segments[3])
            && path_matches(&segments[4..]))
            || (host == "raw.githubusercontent.com"
                && segments.len() >= 3
                && segments[0].eq_ignore_ascii_case(owner)
                && segments[1].eq_ignore_ascii_case(repo)
                && revision_matches(segments[2])
                && path_matches(&segments[3..]))
    } else {
        let repository_len = repository_parts.len();
        host == "gitlab.com"
            && repository_len >= 2
            && segments.len() >= repository_len + 3
            && segments[..repository_len]
                .iter()
                .zip(&repository_parts)
                .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
            && segments[repository_len] == "-"
            && matches!(segments[repository_len + 1], "raw" | "tree")
            && revision_matches(segments[repository_len + 2])
            && path_matches(&segments[repository_len + 3..])
    };
    if !matches {
        return Err(upstream_url_error(
            url,
            "materials URL does not match the configured repository, revision, and path",
        ));
    }
    Ok(())
}

fn repository_identity(value: &str) -> Option<(String, Vec<String>)> {
    let parsed = Url::parse(value).ok()?;
    if parsed.scheme() != "https"
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    if !matches!(host.as_str(), "github.com" | "gitlab.com") {
        return None;
    }
    let mut parts = parsed.path_segments()?.collect::<Vec<_>>();
    if let Some(last) = parts.last_mut() {
        if let Some(stripped) = last.strip_suffix(".git") {
            *last = stripped;
        }
    }
    if parts.len() < 2
        || parts
            .iter()
            .any(|part| part.is_empty() || part.contains('%'))
    {
        return None;
    }
    Some((host, parts.into_iter().map(str::to_owned).collect()))
}

fn immutable_source_url(value: &str) -> Option<Url> {
    let parsed = Url::parse(value).ok()?;
    if parsed.scheme() != "https"
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path().contains('%')
    {
        return None;
    }
    Some(parsed)
}

fn upstream_url_error(url: &str, reason: &str) -> CargoError {
    CargoError::UpstreamEvidence {
        package: "unknown".to_owned(),
        version: "unknown".to_owned(),
        location: url.to_owned(),
        reason: reason.to_owned(),
    }
}

fn verify_manifest(
    package: &Package,
    manifest: &toml::Value,
    manifest_path: &str,
    evidence_path: &str,
    transport: &dyn CargoEvidenceTransport,
    repository: &str,
    revision: &str,
) -> Result<(), CargoError> {
    let package_table = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            upstream_error(
                package,
                manifest_path,
                "manifest has no [package]".to_owned(),
            )
        })?;
    let name = package_table.get("name").and_then(toml::Value::as_str);
    let version = package_table.get("version").and_then(toml::Value::as_str);
    if name != Some(package.name.as_str()) || version != Some(package.version.to_string().as_str())
    {
        return Err(upstream_error(
            package,
            manifest_path,
            "upstream manifest package identity does not match Cargo metadata".to_owned(),
        ));
    }
    let license = package_table.get("license");
    let license_file = package_table
        .get("license-file")
        .and_then(toml::Value::as_str);
    let inherited = license
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("workspace"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    if license.is_none() && license_file.is_none() && !inherited {
        return Err(upstream_error(
            package,
            manifest_path,
            "manifest does not establish license applicability".to_owned(),
        ));
    }
    if inherited {
        let root_request = CargoEvidenceRequest::new(
            immutable_url(repository, revision, "Cargo.toml")?,
            "Cargo.toml".to_owned(),
            package,
            revision,
            MAX_UPSTREAM_RESPONSE_BYTES,
        );
        let root_manifest = fetch_text(transport, &root_request, "workspace manifest")?;
        let root_manifest: toml::Value = toml::from_str(&root_manifest).map_err(|error| {
            upstream_error(
                package,
                "Cargo.toml",
                format!("invalid workspace Cargo.toml: {error}"),
            )
        })?;
        let workspace_license = root_manifest
            .get("workspace")
            .and_then(toml::Value::as_table)
            .and_then(|workspace| workspace.get("package"))
            .and_then(toml::Value::as_table)
            .and_then(|package| {
                package
                    .get("license")
                    .or_else(|| package.get("license-file"))
            });
        if workspace_license.is_none() {
            return Err(upstream_error(
                package,
                "Cargo.toml",
                "workspace inheritance does not establish license applicability".to_owned(),
            ));
        }
    }
    if let Some(license_file) = license_file {
        let expected = join_repo_path(manifest_path, license_file)?;
        if expected != evidence_path {
            return Err(upstream_error(
                package,
                evidence_path,
                "evidence path is not the manifest license-file".to_owned(),
            ));
        }
    }
    Ok(())
}

fn fetch_text(
    transport: &dyn CargoEvidenceTransport,
    request: &CargoEvidenceRequest,
    location: &str,
) -> Result<String, CargoError> {
    let response = transport
        .fetch(request)
        .map_err(|error| upstream_error_fields(request, location, transport_error(error)))?;
    if response.status() != 200 {
        return Err(upstream_error_fields(
            request,
            location,
            format!("upstream returned HTTP {}", response.status()),
        ));
    }
    String::from_utf8(response.body().to_vec()).map_err(|_| {
        upstream_error_fields(
            request,
            location,
            "upstream manifest is not UTF-8".to_owned(),
        )
    })
}

fn push_artifact(
    artifacts: &mut Vec<LicenseArtifact>,
    budget: &mut EvidenceBudget,
    aggregate_limit: u64,
    relative_path: RepoPath,
    bytes: Vec<u8>,
    package: &Package,
) -> Result<(), CargoError> {
    let length = bytes.len() as u64;
    let total = budget
        .aggregate_bytes()
        .checked_add(length)
        .ok_or_else(|| {
            upstream_error(
                package,
                relative_path.as_str(),
                "aggregate limit exceeded".to_owned(),
            )
        })?;
    if total > aggregate_limit {
        return Err(upstream_error(
            package,
            relative_path.as_str(),
            "aggregate limit exceeded".to_owned(),
        ));
    }
    budget.add(length);
    artifacts.push(LicenseArtifact {
        relative_path,
        bytes,
    });
    Ok(())
}

fn immutable_url(repository: &str, revision: &str, path: &str) -> Result<String, CargoError> {
    let normalized = normalize_repository(repository);
    let (host, rest) = normalized
        .strip_prefix("https://")
        .and_then(|value| value.split_once('/'))
        .ok_or_else(|| CargoError::UpstreamEvidence {
            package: "unknown".to_owned(),
            version: "unknown".to_owned(),
            location: repository.to_owned(),
            reason: "repository must be an HTTPS GitHub or GitLab URL".to_owned(),
        })?;
    if host.eq_ignore_ascii_case("github.com") {
        Ok(format!(
            "https://raw.githubusercontent.com/{rest}/{revision}/{path}"
        ))
    } else if host.eq_ignore_ascii_case("gitlab.com") {
        Ok(format!("https://gitlab.com/{rest}/-/raw/{revision}/{path}"))
    } else {
        Err(CargoError::UpstreamEvidence {
            package: "unknown".to_owned(),
            version: "unknown".to_owned(),
            location: repository.to_owned(),
            reason: "repository host is not an allowed provider".to_owned(),
        })
    }
}

fn relative_to_manifest_dir(
    manifest_path: &str,
    evidence_path: &str,
    allow_shared_license: bool,
) -> Result<String, CargoError> {
    let manifest_dir = Path::new(manifest_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    let evidence = Path::new(evidence_path);
    let relative = evidence.strip_prefix(manifest_dir).or_else(|_| {
        if allow_shared_license
            && matches!(
                evidence_path,
                "LICENSE" | "COPYING" | "NOTICE" | "COPYRIGHT"
            )
        {
            Ok(Path::new(evidence_path))
        } else {
            Err(CargoError::UpstreamEvidence {
                package: "unknown".to_owned(),
                version: "unknown".to_owned(),
                location: evidence_path.to_owned(),
                reason: "evidence path is outside package manifest directory".to_owned(),
            })
        }
    })?;
    let value = relative.to_str().unwrap_or_default().replace('\\', "/");
    if value.is_empty() || value.contains("..") {
        return Err(CargoError::UpstreamEvidence {
            package: "unknown".to_owned(),
            version: "unknown".to_owned(),
            location: evidence_path.to_owned(),
            reason: "evidence path is not package-relative".to_owned(),
        });
    }
    Ok(value)
}

fn manifest_inherits_license(manifest: &toml::Value) -> bool {
    manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("license"))
        .and_then(toml::Value::as_table)
        .and_then(|license| license.get("workspace"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false)
}

fn join_repo_path(manifest_path: &str, value: &str) -> Result<String, CargoError> {
    let manifest = RepoPath::parse(manifest_path).map_err(|_| CargoError::UpstreamEvidence {
        package: "unknown".to_owned(),
        version: "unknown".to_owned(),
        location: manifest_path.to_owned(),
        reason: "upstream manifest path is not repository-safe".to_owned(),
    })?;
    let relative = RepoPath::parse(value).map_err(|_| CargoError::UpstreamEvidence {
        package: "unknown".to_owned(),
        version: "unknown".to_owned(),
        location: value.to_owned(),
        reason: "manifest license-file escapes package scope".to_owned(),
    })?;
    let parent = manifest
        .as_str()
        .rsplit_once('/')
        .map_or("", |(parent, _)| parent);
    let result = if parent.is_empty() {
        relative.as_str().to_owned()
    } else {
        format!("{parent}/{}", relative.as_str())
    };
    if RepoPath::parse(&result).is_err() {
        return Err(CargoError::UpstreamEvidence {
            package: "unknown".to_owned(),
            version: "unknown".to_owned(),
            location: value.to_owned(),
            reason: "manifest license-file escapes package scope".to_owned(),
        });
    }
    Ok(result)
}

fn normalize_repository(value: &str) -> String {
    value
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_ascii_lowercase()
}

fn is_allowed_repository(value: &str) -> bool {
    let normalized = normalize_repository(value);
    normalized.starts_with("https://github.com/") || normalized.starts_with("https://gitlab.com/")
}

fn is_revision(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64)
        && value.chars().all(|character| character.is_ascii_hexdigit())
}

fn transport_error(error: CargoTransportError) -> String {
    match error {
        CargoTransportError::RequestFailed => "upstream request failed".to_owned(),
        CargoTransportError::ResponseTooLarge => {
            "upstream response exceeded the bounded limit".to_owned()
        }
    }
}

fn upstream_error(package: &Package, location: &str, reason: String) -> CargoError {
    CargoError::UpstreamEvidence {
        package: package.name.to_string(),
        version: package.version.to_string(),
        location: location.to_owned(),
        reason,
    }
}

fn upstream_error_fields(
    request: &CargoEvidenceRequest,
    location: &str,
    reason: String,
) -> CargoError {
    CargoError::UpstreamEvidence {
        package: request.package.clone(),
        version: request.version.clone(),
        location: location.to_owned(),
        reason,
    }
}
