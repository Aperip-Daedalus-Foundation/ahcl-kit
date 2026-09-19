// crates/ahcl-kit-license/src/verify.rs - Official AHCL record and digest verification.
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

use crate::{LicenseError, LicenseErrorCode, LicenseResponse, VerifiedLicense};
use ahcl_kit_config::AhclVersion;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

pub(crate) const OFFICIAL_ORIGIN: &str = "https://ahcl.aperip.com";

pub(crate) struct OfficialRecord {
    pub(crate) version: AhclVersion,
    pub(crate) slug: &'static str,
    pub(crate) title: &'static str,
    pub(crate) source_filename: &'static str,
    pub(crate) sha256: &'static str,
}

pub(crate) fn official_record(version: AhclVersion) -> OfficialRecord {
    match version {
        AhclVersion::V1_0 => OfficialRecord {
            version,
            slug: "ahcl-1-0",
            title: "Aperip Heimdall Commons License 1.0",
            source_filename: "AHCL-1.0.md",
            sha256: "01c51c190a021cedcd072fdb2a7da1857bf5ef9a8770d26104aa472455ac003e",
        },
        AhclVersion::V1_1 => OfficialRecord {
            version,
            slug: "ahcl-1-1",
            title: "Aperip Heimdall Commons License 1.1",
            source_filename: "AHCL-1.1.md",
            sha256: "41bfa8d3621494b84ba87a0b2a07748be339c10146787fe0d89ef7c38358d702",
        },
    }
}

#[derive(Deserialize)]
struct ResponseDocument {
    license: ResponseLicense,
}

#[derive(Deserialize)]
struct ResponseLicense {
    slug: String,
    title: String,
    body: String,
    body_format: String,
    source_filename: String,
    sha256: String,
}

pub(crate) fn verify_response(
    record: &OfficialRecord,
    response: LicenseResponse,
    response_limit: usize,
) -> Result<VerifiedLicense, LicenseError> {
    if response.redirected() || !has_official_origin(response.final_url()) {
        return failure(LicenseErrorCode::RedirectOrigin);
    }
    if response.status() != 200 {
        return failure(LicenseErrorCode::Status);
    }
    if !is_json_content_type(response.content_type()) {
        return failure(LicenseErrorCode::ContentType);
    }
    if response.body().len() > response_limit {
        return failure(LicenseErrorCode::ResponseSize);
    }

    let document: ResponseDocument = serde_json::from_slice(response.body())
        .map_err(|_| LicenseError::new(LicenseErrorCode::JsonShape))?;
    let license = document.license;
    if license.slug != record.slug
        || license.title != record.title
        || license.source_filename != record.source_filename
        || license.body_format != "markdown"
    {
        return failure(LicenseErrorCode::MetadataMismatch);
    }
    if !is_lowercase_sha256(&license.sha256) {
        return failure(LicenseErrorCode::DigestMismatch);
    }

    let body_digest = sha256(license.body.as_bytes());
    if body_digest != license.sha256 || body_digest != record.sha256 {
        return failure(LicenseErrorCode::DigestMismatch);
    }

    Ok(VerifiedLicense {
        version: record.version,
        slug: license.slug,
        title: license.title,
        source_filename: license.source_filename,
        body: license.body,
        sha256: license.sha256,
    })
}

fn has_official_origin(value: &str) -> bool {
    Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("ahcl.aperip.com")
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
    })
}

fn is_json_content_type(value: Option<&str>) -> bool {
    value
        .and_then(|value| value.parse::<mime::Mime>().ok())
        .is_some_and(|value| {
            value.type_().as_str().eq_ignore_ascii_case("application")
                && value.subtype().as_str().eq_ignore_ascii_case("json")
        })
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn failure<T>(code: LicenseErrorCode) -> Result<T, LicenseError> {
    Err(LicenseError::new(code))
}
