// crates/ahcl-kit-cargo/src/upstream_manifest.rs - Cargo manifest and revision checks.
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

use crate::adapter::CargoError;
use crate::settings::{CargoEvidence, CargoEvidenceKind};
use crate::upstream::{
    CargoEvidenceRequest, CargoEvidenceTransport, CargoTransportError, MAX_UPSTREAM_RESPONSE_BYTES,
    upstream_error, upstream_error_fields,
};
use crate::upstream_url::{validate_materials_url, validate_source_url};
use ahcl_kit_core::RepoPath;
use cargo_metadata::Package;
use serde_json::Value as JsonValue;
use std::path::Path;

pub(crate) fn exact_revision(
    package: &Package,
    mapping: Option<&CargoEvidence>,
) -> Result<String, CargoError> {
    if let Some(revision) = revision_from_source(package)? {
        return Ok(revision);
    }
    revision_from_vcs(package, mapping)
}

fn revision_from_source(package: &Package) -> Result<Option<String>, CargoError> {
    let Some(source) = package.source.as_ref() else {
        return Ok(None);
    };
    let source = source.to_string();
    let Some(fragment) = source.rsplit_once('#').map(|(_, value)| value.to_owned()) else {
        return Ok(None);
    };
    accept_source_fragment(package, &source, fragment)
}

fn accept_source_fragment(
    package: &Package,
    source: &str,
    fragment: String,
) -> Result<Option<String>, CargoError> {
    if is_revision(&fragment) {
        return Ok(Some(fragment));
    }
    if source.starts_with("git+") {
        return Err(upstream_error(
            package,
            "source",
            "Cargo git source does not contain a full commit revision".to_owned(),
        ));
    }
    Ok(None)
}

fn revision_from_vcs(
    package: &Package,
    mapping: Option<&CargoEvidence>,
) -> Result<String, CargoError> {
    let root = Path::new(package.manifest_path.as_std_path())
        .parent()
        .ok_or_else(|| {
            upstream_error(package, "source", "package root is unavailable".to_owned())
        })?;
    let vcs = root.join(".cargo_vcs_info.json");
    match std::fs::read(&vcs) {
        Ok(bytes) => revision_from_metadata(package, mapping, &bytes),
        Err(error) => revision_from_vcs_error(package, mapping, &vcs, error),
    }
}

fn revision_from_vcs_error(
    package: &Package,
    mapping: Option<&CargoEvidence>,
    vcs: &Path,
    error: std::io::Error,
) -> Result<String, CargoError> {
    if error.kind() == std::io::ErrorKind::NotFound {
        if let Some(mapping) = mapping.filter(|mapping| is_revision(mapping.revision())) {
            return Ok(mapping.revision().to_owned());
        }
    }
    Err(upstream_error(
        package,
        &vcs.to_string_lossy(),
        format!("exact revision unavailable: {error}"),
    ))
}

fn revision_from_metadata(
    package: &Package,
    mapping: Option<&CargoEvidence>,
    bytes: &[u8],
) -> Result<String, CargoError> {
    let value: JsonValue = serde_json::from_slice(bytes).map_err(|error| {
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

pub(crate) fn upstream_manifest_path(
    package: &Package,
    evidence_path: &str,
) -> Result<String, CargoError> {
    let root = Path::new(package.manifest_path.as_std_path())
        .parent()
        .ok_or_else(|| {
            upstream_error(
                package,
                evidence_path,
                "package root unavailable".to_owned(),
            )
        })?;
    if let Some(path) = vcs_manifest_path(package, root)? {
        return Ok(path);
    }
    Ok(evidence_parent_manifest(evidence_path))
}

fn vcs_manifest_path(package: &Package, root: &Path) -> Result<Option<String>, CargoError> {
    let vcs = root.join(".cargo_vcs_info.json");
    match std::fs::read(&vcs) {
        Ok(bytes) => manifest_path_in_vcs(package, &bytes),
        Err(error) => missing_or_unreadable_vcs(package, &vcs, error),
    }
}

fn missing_or_unreadable_vcs(
    package: &Package,
    vcs: &Path,
    error: std::io::Error,
) -> Result<Option<String>, CargoError> {
    if error.kind() == std::io::ErrorKind::NotFound {
        return Ok(None);
    }
    Err(upstream_error(
        package,
        &vcs.to_string_lossy(),
        format!("cannot read metadata: {error}"),
    ))
}

fn manifest_path_in_vcs(package: &Package, bytes: &[u8]) -> Result<Option<String>, CargoError> {
    let value = serde_json::from_slice::<JsonValue>(bytes).map_err(|error| {
        upstream_error(
            package,
            ".cargo_vcs_info.json",
            format!("invalid metadata: {error}"),
        )
    })?;
    let Some(path) = value.get("path_in_vcs").and_then(JsonValue::as_str) else {
        return Ok(None);
    };
    accept_vcs_path(package, path)
}

fn accept_vcs_path(package: &Package, path: &str) -> Result<Option<String>, CargoError> {
    let path = path.trim_matches('/');
    if path.is_empty() || unsafe_vcs_part(path) {
        return Err(upstream_error(
            package,
            ".cargo_vcs_info.json",
            "metadata path_in_vcs is not repository-safe".to_owned(),
        ));
    }
    Ok(Some(format!("{path}/Cargo.toml")))
}

fn unsafe_vcs_part(path: &str) -> bool {
    path.split('/').any(|part| part == ".." || part.is_empty())
}

fn evidence_parent_manifest(evidence_path: &str) -> String {
    let parent = Path::new(evidence_path)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or_default();
    if parent.is_empty() {
        "Cargo.toml".to_owned()
    } else {
        format!("{parent}/Cargo.toml")
    }
}

pub(crate) fn manifest_license_candidate(
    package: &Package,
    manifest: &toml::Value,
    manifest_path: &str,
    transport: &dyn CargoEvidenceTransport,
    repository: &str,
    revision: &str,
) -> Result<Option<String>, CargoError> {
    let context = UpstreamManifest {
        package,
        manifest_path,
        transport,
        repository,
        revision,
    };
    license_candidate(&context, manifest)
}

pub(crate) fn validate_mapping(
    package: &Package,
    source: &str,
    mapping: &CargoEvidence,
    revision: &str,
) -> Result<(), CargoError> {
    reject_mapping_source(package, source, mapping)?;
    reject_mapping_revision(package, mapping, revision)?;
    reject_mapping_repository(package, mapping)?;
    validate_mapping_url(mapping, revision)
}

fn reject_mapping_source(
    package: &Package,
    source: &str,
    mapping: &CargoEvidence,
) -> Result<(), CargoError> {
    if mapping.source() != source {
        return Err(upstream_error(
            package,
            mapping.path().as_str(),
            "evidence source does not match Cargo metadata".to_owned(),
        ));
    }
    Ok(())
}

fn reject_mapping_revision(
    package: &Package,
    mapping: &CargoEvidence,
    revision: &str,
) -> Result<(), CargoError> {
    if !is_revision(revision) || mapping.revision() != revision {
        return Err(upstream_error(
            package,
            mapping.path().as_str(),
            "evidence revision does not match Cargo metadata".to_owned(),
        ));
    }
    reject_source_revision(package, mapping, revision)
}

fn reject_source_revision(
    package: &Package,
    mapping: &CargoEvidence,
    revision: &str,
) -> Result<(), CargoError> {
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
    Ok(())
}

fn reject_mapping_repository(package: &Package, mapping: &CargoEvidence) -> Result<(), CargoError> {
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
    Ok(())
}

fn validate_mapping_url(mapping: &CargoEvidence, revision: &str) -> Result<(), CargoError> {
    if mapping.kind() == CargoEvidenceKind::Materials {
        return validate_materials_url(
            mapping.url(),
            mapping.repository(),
            revision,
            mapping.path().as_str(),
        );
    }
    validate_source_url(
        mapping.url(),
        mapping.repository(),
        revision,
        mapping.path().as_str(),
    )
}

pub(crate) struct UpstreamManifest<'a> {
    pub(crate) package: &'a Package,
    pub(crate) manifest_path: &'a str,
    pub(crate) transport: &'a dyn CargoEvidenceTransport,
    pub(crate) repository: &'a str,
    pub(crate) revision: &'a str,
}

pub(crate) fn verify_manifest(
    context: &UpstreamManifest<'_>,
    manifest: &toml::Value,
    evidence_path: &str,
) -> Result<(), CargoError> {
    verify_manifest_fields(context, manifest, evidence_path)
}

struct LicenseFields<'a> {
    license: Option<&'a toml::Value>,
    license_file: Option<&'a str>,
    inherited: bool,
}

fn license_candidate(
    context: &UpstreamManifest<'_>,
    manifest: &toml::Value,
) -> Result<Option<String>, CargoError> {
    let table = package_table(context, manifest)?;
    reject_package_identity(context, table)?;
    let state = license_fields(table);
    reject_without_applicability(context, &state)?;
    // Inherited workspace license is checked before a package file is accepted.
    confirm_inherited_license(context, state.inherited)?;
    if let Some(license_file) = state.license_file {
        return joined_license_file(context, license_file);
    }
    probe_license_names(context)
}

fn verify_manifest_fields(
    context: &UpstreamManifest<'_>,
    manifest: &toml::Value,
    evidence_path: &str,
) -> Result<(), CargoError> {
    let table = package_table(context, manifest)?;
    reject_package_identity(context, table)?;
    let state = license_fields(table);
    reject_without_applicability(context, &state)?;
    confirm_inherited_license(context, state.inherited)?;
    match state.license_file {
        Some(license_file) => reject_unexpected_license_file(context, evidence_path, license_file),
        None => Ok(()),
    }
}

fn package_table<'a>(
    context: &UpstreamManifest<'_>,
    manifest: &'a toml::Value,
) -> Result<&'a toml::Table, CargoError> {
    manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| {
            upstream_error(
                context.package,
                context.manifest_path,
                "manifest has no [package]".to_owned(),
            )
        })
}

fn reject_package_identity(
    context: &UpstreamManifest<'_>,
    table: &toml::Table,
) -> Result<(), CargoError> {
    let name = table.get("name").and_then(toml::Value::as_str);
    let version = table.get("version").and_then(toml::Value::as_str);
    if name != Some(context.package.name.as_str())
        || version != Some(context.package.version.to_string().as_str())
    {
        return Err(upstream_error(
            context.package,
            context.manifest_path,
            "upstream manifest package identity does not match Cargo metadata".to_owned(),
        ));
    }
    Ok(())
}

fn license_fields(table: &toml::Table) -> LicenseFields<'_> {
    let license = table.get("license");
    let license_file = table.get("license-file").and_then(toml::Value::as_str);
    let inherited = license
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("workspace"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    LicenseFields {
        license,
        license_file,
        inherited,
    }
}

fn reject_without_applicability(
    context: &UpstreamManifest<'_>,
    state: &LicenseFields<'_>,
) -> Result<(), CargoError> {
    if state.license.is_none() && state.license_file.is_none() && !state.inherited {
        return Err(upstream_error(
            context.package,
            context.manifest_path,
            "manifest does not establish license applicability".to_owned(),
        ));
    }
    Ok(())
}

fn confirm_inherited_license(
    context: &UpstreamManifest<'_>,
    inherited: bool,
) -> Result<(), CargoError> {
    if !inherited {
        return Ok(());
    }
    let root_manifest = workspace_manifest(context)?;
    if workspace_license(&root_manifest).is_none() {
        return Err(upstream_error(
            context.package,
            "Cargo.toml",
            "workspace inheritance does not establish license applicability".to_owned(),
        ));
    }
    Ok(())
}

fn workspace_manifest(context: &UpstreamManifest<'_>) -> Result<toml::Value, CargoError> {
    let root_request = CargoEvidenceRequest::new(
        immutable_url(context.repository, context.revision, "Cargo.toml")?,
        "Cargo.toml".to_owned(),
        context.package,
        context.revision,
        MAX_UPSTREAM_RESPONSE_BYTES,
    );
    let root_manifest = fetch_text(context.transport, &root_request, "workspace manifest")?;
    toml::from_str(&root_manifest).map_err(|error| {
        upstream_error(
            context.package,
            "Cargo.toml",
            format!("invalid workspace Cargo.toml: {error}"),
        )
    })
}

fn workspace_license(root_manifest: &toml::Value) -> Option<&toml::Value> {
    root_manifest
        .get("workspace")
        .and_then(toml::Value::as_table)
        .and_then(|workspace| workspace.get("package"))
        .and_then(toml::Value::as_table)
        .and_then(|package| {
            package
                .get("license")
                .or_else(|| package.get("license-file"))
        })
}

fn joined_license_file(
    context: &UpstreamManifest<'_>,
    license_file: &str,
) -> Result<Option<String>, CargoError> {
    Ok(Some(join_repo_path(context.manifest_path, license_file)?))
}

fn reject_unexpected_license_file(
    context: &UpstreamManifest<'_>,
    evidence_path: &str,
    license_file: &str,
) -> Result<(), CargoError> {
    let expected = join_repo_path(context.manifest_path, license_file)?;
    if expected != evidence_path {
        return Err(upstream_error(
            context.package,
            evidence_path,
            "evidence path is not the manifest license-file".to_owned(),
        ));
    }
    Ok(())
}

fn probe_license_names(context: &UpstreamManifest<'_>) -> Result<Option<String>, CargoError> {
    let parent = Path::new(context.manifest_path)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or_default();
    for name in ["LICENSE", "COPYING", "COPYRIGHT"] {
        if let Some(path) = probe_license_name(context, parent, name)? {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn probe_license_name(
    context: &UpstreamManifest<'_>,
    parent: &str,
    name: &str,
) -> Result<Option<String>, CargoError> {
    let path = if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    };
    let request = CargoEvidenceRequest::new(
        immutable_url(context.repository, context.revision, &path)?,
        path.clone(),
        context.package,
        context.revision,
        MAX_UPSTREAM_RESPONSE_BYTES,
    );
    let response = context
        .transport
        .fetch(&request)
        .map_err(|error| upstream_error(context.package, &path, transport_error(error)))?;
    if response.status() == 200 {
        return Ok(Some(path));
    }
    Ok(None)
}

pub(crate) fn fetch_text(
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

pub(crate) fn immutable_url(
    repository: &str,
    revision: &str,
    path: &str,
) -> Result<String, CargoError> {
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

pub(crate) fn relative_to_manifest_dir(
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

pub(crate) fn manifest_inherits_license(manifest: &toml::Value) -> bool {
    manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("license"))
        .and_then(toml::Value::as_table)
        .and_then(|license| license.get("workspace"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn join_repo_path(manifest_path: &str, value: &str) -> Result<String, CargoError> {
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

pub(crate) fn is_allowed_repository(value: &str) -> bool {
    let normalized = normalize_repository(value);
    normalized.starts_with("https://github.com/") || normalized.starts_with("https://gitlab.com/")
}

fn is_revision(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64)
        && value.chars().all(|character| character.is_ascii_hexdigit())
}

pub(crate) fn transport_error(error: CargoTransportError) -> String {
    match error {
        CargoTransportError::RequestFailed => "upstream request failed".to_owned(),
        CargoTransportError::ResponseTooLarge => {
            "upstream response exceeded the bounded limit".to_owned()
        }
    }
}
