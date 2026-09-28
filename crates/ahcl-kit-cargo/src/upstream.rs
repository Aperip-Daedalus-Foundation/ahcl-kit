// crates/ahcl-kit-cargo/src/upstream.rs - Upstream license evidence recovery.
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

use crate::adapter::{CargoError, CargoResolveRequest};
use crate::collector::EvidenceBudget;
use crate::settings::{CargoEvidence, CargoEvidenceKind};
use crate::upstream_manifest::{
    UpstreamManifest, exact_revision, fetch_text, immutable_url, is_allowed_repository,
    manifest_inherits_license, manifest_license_candidate, relative_to_manifest_dir,
    transport_error, upstream_manifest_path, validate_mapping, verify_manifest,
};
use crate::upstream_url::validate_source_url;
use ahcl_kit_core::{LicenseArtifact, RepoPath};
use cargo_metadata::Package;
use std::time::Duration;

pub(crate) const MAX_UPSTREAM_RESPONSE_BYTES: u64 = 2_097_152;

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
    pub(crate) fn new(
        url: String,
        path: String,
        package: &Package,
        revision: &str,
        max_bytes: u64,
    ) -> Self {
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
    let source = package_source(package);
    let mapping = license_mapping(request, package, &source);
    if unmapped_repository_rejected(package, mapping.is_some()) {
        return Ok(Vec::new());
    }
    let revision = exact_revision(package, mapping)?;
    if let Some(mapping) = mapping {
        validate_mapping(package, &source, mapping, &revision)?;
    }
    recover_mapped_license(
        RecoveryInput {
            package,
            request,
            transport,
            budget,
            existing,
        },
        mapping,
        &revision,
    )
}

fn package_source(package: &Package) -> String {
    package
        .source
        .as_ref()
        .map_or_else(|| "path".to_owned(), ToString::to_string)
}

fn license_mapping<'a>(
    request: &'a CargoResolveRequest,
    package: &Package,
    source: &str,
) -> Option<&'a CargoEvidence> {
    request.settings().and_then(|settings| {
        find_mapping(
            settings.evidence(),
            package,
            source,
            CargoEvidenceKind::License,
        )
    })
}

fn unmapped_repository_rejected(package: &Package, mapped: bool) -> bool {
    !mapped
        && !package
            .repository
            .as_deref()
            .is_some_and(is_allowed_repository)
}

fn recover_mapped_license(
    input: RecoveryInput<'_>,
    mapping: Option<&CargoEvidence>,
    revision: &str,
) -> Result<Vec<LicenseArtifact>, CargoError> {
    let Some(repository) = resolved_repository(input.package, mapping) else {
        return Ok(Vec::new());
    };
    let manifest = load_upstream_manifest(
        input.package,
        mapping,
        repository,
        revision,
        input.transport,
    )?;
    let evidence_path = select_evidence_path(
        input.package,
        mapping,
        &manifest,
        input.transport,
        repository,
        revision,
    )?;
    LicenseRecovery {
        package: input.package,
        request: input.request,
        transport: input.transport,
        budget: input.budget,
        existing: input.existing,
        mapping,
        repository,
        revision,
    }
    .finish(&manifest, evidence_path)
}

struct RecoveryInput<'a> {
    package: &'a Package,
    request: &'a CargoResolveRequest,
    transport: &'a dyn CargoEvidenceTransport,
    budget: &'a mut EvidenceBudget,
    existing: &'a [LicenseArtifact],
}

fn resolved_repository<'a>(
    package: &'a Package,
    mapping: Option<&'a CargoEvidence>,
) -> Option<&'a str> {
    package
        .repository
        .as_deref()
        .or_else(|| mapping.map(|item| item.repository()))
}

struct LoadedManifest {
    path: String,
    value: toml::Value,
}

fn load_upstream_manifest(
    package: &Package,
    mapping: Option<&CargoEvidence>,
    repository: &str,
    revision: &str,
    transport: &dyn CargoEvidenceTransport,
) -> Result<LoadedManifest, CargoError> {
    let manifest_hint = mapping.map_or("Cargo.toml", |mapping| mapping.path().as_str());
    let manifest_path = upstream_manifest_path(package, manifest_hint)?;
    let manifest_url = immutable_url(repository, revision, &manifest_path)?;
    let manifest_request = CargoEvidenceRequest::new(
        manifest_url,
        manifest_path.clone(),
        package,
        revision,
        MAX_UPSTREAM_RESPONSE_BYTES,
    );
    let manifest = fetch_text(transport, &manifest_request, "manifest")?;
    let value = toml::from_str(&manifest).map_err(|error| {
        upstream_error(package, "manifest", format!("invalid Cargo.toml: {error}"))
    })?;
    Ok(LoadedManifest {
        path: manifest_path,
        value,
    })
}

fn select_evidence_path(
    package: &Package,
    mapping: Option<&CargoEvidence>,
    manifest: &LoadedManifest,
    transport: &dyn CargoEvidenceTransport,
    repository: &str,
    revision: &str,
) -> Result<Option<String>, CargoError> {
    if let Some(mapping) = mapping {
        // A configured evidence path is accepted only after the manifest identity
        // and license-file checks succeed.
        verify_manifest(
            &UpstreamManifest {
                package,
                manifest_path: &manifest.path,
                transport,
                repository,
                revision,
            },
            &manifest.value,
            mapping.path().as_str(),
        )?;
        return Ok(Some(mapping.path().as_str().to_owned()));
    }
    manifest_license_candidate(
        package,
        &manifest.value,
        &manifest.path,
        transport,
        repository,
        revision,
    )
}

struct LicenseRecovery<'a> {
    package: &'a Package,
    request: &'a CargoResolveRequest,
    transport: &'a dyn CargoEvidenceTransport,
    budget: &'a mut EvidenceBudget,
    existing: &'a [LicenseArtifact],
    mapping: Option<&'a CargoEvidence>,
    repository: &'a str,
    revision: &'a str,
}

impl LicenseRecovery<'_> {
    fn finish(
        &mut self,
        manifest: &LoadedManifest,
        evidence_path: Option<String>,
    ) -> Result<Vec<LicenseArtifact>, CargoError> {
        let Some(evidence_path) = evidence_path else {
            return Ok(Vec::new());
        };
        let evidence_url = self.evidence_url(&evidence_path)?;
        self.validate_evidence_url(&evidence_url)?;
        let response = self.fetch_evidence(&evidence_url, &evidence_path)?;
        self.record(manifest, &evidence_path, &evidence_url, response)
    }

    fn evidence_url(&self, evidence_path: &str) -> Result<String, CargoError> {
        match self.mapping {
            Some(mapping) => Ok(mapping.url().to_owned()),
            None => immutable_url(self.repository, self.revision, evidence_path),
        }
    }

    fn validate_evidence_url(&self, evidence_url: &str) -> Result<(), CargoError> {
        let Some(mapping) = self.mapping else {
            return Ok(());
        };
        validate_source_url(
            evidence_url,
            mapping.repository(),
            self.revision,
            mapping.path().as_str(),
        )
    }

    fn fetch_evidence(
        &self,
        evidence_url: &str,
        evidence_path: &str,
    ) -> Result<CargoEvidenceResponse, CargoError> {
        let evidence_request = CargoEvidenceRequest::new(
            evidence_url.to_owned(),
            evidence_path.to_owned(),
            self.package,
            self.revision,
            MAX_UPSTREAM_RESPONSE_BYTES,
        );
        let response = self
            .transport
            .fetch(&evidence_request)
            .map_err(|error| upstream_error(self.package, evidence_path, transport_error(error)))?;
        if response.status() != 200 {
            return Err(upstream_error(
                self.package,
                evidence_path,
                format!("upstream returned HTTP {}", response.status()),
            ));
        }
        Ok(response)
    }

    fn record(
        &mut self,
        manifest: &LoadedManifest,
        evidence_path: &str,
        evidence_url: &str,
        response: CargoEvidenceResponse,
    ) -> Result<Vec<LicenseArtifact>, CargoError> {
        let package_relative = relative_to_manifest_dir(
            &manifest.path,
            evidence_path,
            manifest_inherits_license(&manifest.value),
        )?;
        let relative = RepoPath::parse(&package_relative).map_err(|_| {
            upstream_error(
                self.package,
                evidence_path,
                "evidence path is not repository-safe".to_owned(),
            )
        })?;
        let mut recovered = Vec::new();
        push_artifact(
            &mut recovered,
            self.budget,
            self.request.limits().max_aggregate_bytes(),
            relative,
            response.body().to_vec(),
            self.package,
        )?;
        self.append_source_note(&mut recovered, evidence_url)?;
        Ok(recovered)
    }

    fn append_source_note(
        &mut self,
        recovered: &mut Vec<LicenseArtifact>,
        evidence_url: &str,
    ) -> Result<(), CargoError> {
        if self
            .existing
            .iter()
            .any(|artifact| artifact.relative_path.as_str() == "AHCL-EVIDENCE-SOURCE.md")
        {
            return Ok(());
        }
        let source_note =
            format!("License files are sourced from the owning repository.\n<{evidence_url}>\n");
        push_artifact(
            recovered,
            self.budget,
            self.request.limits().max_aggregate_bytes(),
            RepoPath::parse("AHCL-EVIDENCE-SOURCE.md").expect("static path"),
            source_note.into_bytes(),
            self.package,
        )
    }
}

pub(crate) fn supplement_materials(
    package: &Package,
    request: &CargoResolveRequest,
    budget: &mut EvidenceBudget,
    existing: &[LicenseArtifact],
) -> Result<Vec<LicenseArtifact>, CargoError> {
    let Some(mapping) = materials_mapping(package, request) else {
        return Ok(Vec::new());
    };
    accept_materials_mapping(package, mapping)?;
    if has_materials_shortcut(existing) {
        return Ok(Vec::new());
    }
    materials_shortcut(package, request, budget, mapping)
}

fn materials_mapping<'a>(
    package: &Package,
    request: &'a CargoResolveRequest,
) -> Option<&'a CargoEvidence> {
    let source = package_source(package);
    let mapping = request.settings().and_then(|settings| {
        find_mapping(
            settings.evidence(),
            package,
            &source,
            CargoEvidenceKind::Materials,
        )
    })?;
    if mapping.kind() == CargoEvidenceKind::Materials {
        Some(mapping)
    } else {
        None
    }
}

fn accept_materials_mapping(package: &Package, mapping: &CargoEvidence) -> Result<(), CargoError> {
    let source = package_source(package);
    let revision = exact_revision(package, Some(mapping))?;
    validate_mapping(package, &source, mapping, &revision)
}

fn has_materials_shortcut(existing: &[LicenseArtifact]) -> bool {
    existing
        .iter()
        .any(|artifact| artifact.relative_path.as_str() == "AHCL-MATERIALS.url")
}

fn materials_shortcut(
    package: &Package,
    request: &CargoResolveRequest,
    budget: &mut EvidenceBudget,
    mapping: &CargoEvidence,
) -> Result<Vec<LicenseArtifact>, CargoError> {
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
    let Some(mapping) = notice_mapping(package, request) else {
        return Ok(Vec::new());
    };
    let located = locate_notice(package, mapping)?;
    if notice_already_present(existing) {
        return Ok(Vec::new());
    }
    let response = fetch_notice(package, transport, mapping, &located)?;
    let mut recovered = Vec::new();
    let mut write = NoticeWrite {
        package,
        request,
        budget,
        mapping,
        located: &located,
        existing,
    };
    record_notice(&mut write, response.body(), &mut recovered)?;
    Ok(recovered)
}

fn notice_mapping<'a>(
    package: &Package,
    request: &'a CargoResolveRequest,
) -> Option<&'a CargoEvidence> {
    let source = package_source(package);
    request.settings().and_then(|settings| {
        find_mapping(
            settings.evidence(),
            package,
            &source,
            CargoEvidenceKind::Notice,
        )
    })
}

struct LocatedNotice {
    revision: String,
    manifest_path: String,
    evidence_url: String,
}

fn locate_notice(package: &Package, mapping: &CargoEvidence) -> Result<LocatedNotice, CargoError> {
    let revision = prepared_notice_revision(package, mapping)?;
    let manifest_path = notice_manifest(package, mapping, &revision)?;
    Ok(LocatedNotice {
        evidence_url: mapping.url().to_owned(),
        revision,
        manifest_path,
    })
}

fn prepared_notice_revision(
    package: &Package,
    mapping: &CargoEvidence,
) -> Result<String, CargoError> {
    let source = package_source(package);
    let revision = exact_revision(package, Some(mapping))?;
    validate_mapping(package, &source, mapping, &revision)?;
    Ok(revision)
}

fn notice_manifest(
    package: &Package,
    mapping: &CargoEvidence,
    revision: &str,
) -> Result<String, CargoError> {
    let repository = package
        .repository
        .as_deref()
        .unwrap_or(mapping.repository());
    let manifest_path = upstream_manifest_path(package, mapping.path().as_str())?;
    validate_source_url(mapping.url(), repository, revision, mapping.path().as_str())?;
    Ok(manifest_path)
}

fn notice_already_present(existing: &[LicenseArtifact]) -> bool {
    existing.iter().any(|artifact| {
        artifact
            .relative_path
            .as_path()
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_uppercase().starts_with("NOTICE"))
    })
}

fn fetch_notice(
    package: &Package,
    transport: &dyn CargoEvidenceTransport,
    mapping: &CargoEvidence,
    located: &LocatedNotice,
) -> Result<CargoEvidenceResponse, CargoError> {
    let evidence_request = CargoEvidenceRequest::new(
        located.evidence_url.clone(),
        mapping.path().as_str().to_owned(),
        package,
        &located.revision,
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
    Ok(response)
}

struct NoticeWrite<'a> {
    package: &'a Package,
    request: &'a CargoResolveRequest,
    budget: &'a mut EvidenceBudget,
    mapping: &'a CargoEvidence,
    located: &'a LocatedNotice,
    existing: &'a [LicenseArtifact],
}

fn record_notice(
    write: &mut NoticeWrite<'_>,
    body: &[u8],
    recovered: &mut Vec<LicenseArtifact>,
) -> Result<(), CargoError> {
    let relative = notice_repo_path(write.package, write.mapping, &write.located.manifest_path)?;
    push_artifact(
        recovered,
        write.budget,
        write.request.limits().max_aggregate_bytes(),
        relative,
        body.to_vec(),
        write.package,
    )?;
    push_notice_source(write, recovered)
}

fn notice_repo_path(
    package: &Package,
    mapping: &CargoEvidence,
    manifest_path: &str,
) -> Result<RepoPath, CargoError> {
    let package_relative = relative_to_manifest_dir(manifest_path, mapping.path().as_str(), true)?;
    RepoPath::parse(&package_relative).map_err(|_| {
        upstream_error(
            package,
            mapping.path().as_str(),
            "evidence path is not repository-safe".to_owned(),
        )
    })
}

fn push_notice_source(
    write: &mut NoticeWrite<'_>,
    recovered: &mut Vec<LicenseArtifact>,
) -> Result<(), CargoError> {
    if write
        .existing
        .iter()
        .any(|artifact| artifact.relative_path.as_str() == "AHCL-EVIDENCE-SOURCE.md")
    {
        return Ok(());
    }
    let source_note = format!(
        "License files are sourced from the owning repository.\n<{}>\n",
        write.located.evidence_url
    );
    push_artifact(
        recovered,
        write.budget,
        write.request.limits().max_aggregate_bytes(),
        RepoPath::parse("AHCL-EVIDENCE-SOURCE.md").expect("static path"),
        source_note.into_bytes(),
        write.package,
    )
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

pub(crate) fn upstream_error(package: &Package, location: &str, reason: String) -> CargoError {
    CargoError::UpstreamEvidence {
        package: package.name.to_string(),
        version: package.version.to_string(),
        location: location.to_owned(),
        reason,
    }
}

pub(crate) fn upstream_error_fields(
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
