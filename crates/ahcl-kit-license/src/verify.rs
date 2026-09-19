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

const OFFICIAL_ORIGIN: &str = "https://ahcl.aperip.com";

pub(crate) struct OfficialRecord {
    pub(crate) version: AhclVersion,
    pub(crate) slug: &'static str,
    pub(crate) title: &'static str,
    pub(crate) source_filename: &'static str,
    pub(crate) sha256: &'static str,
}

impl OfficialRecord {
    fn endpoint_path(&self) -> String {
        format!("/api/licenses/{}", self.slug)
    }

    pub(crate) fn endpoint_url(&self) -> Url {
        let mut endpoint =
            Url::parse(OFFICIAL_ORIGIN).expect("the built-in official origin must be valid");
        endpoint.set_path(&self.endpoint_path());
        endpoint
    }

    fn matches_endpoint(&self, value: &str) -> bool {
        let endpoint = self.endpoint_url();
        Url::parse(value).is_ok_and(|observed| {
            observed.scheme() == endpoint.scheme()
                && observed.host_str() == endpoint.host_str()
                && observed.port_or_known_default() == endpoint.port_or_known_default()
                && observed.username().is_empty()
                && observed.password().is_none()
                && observed.path() == endpoint.path()
                && observed.query().is_none()
                && observed.fragment().is_none()
        })
    }
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
    if response.redirected() || !record.matches_endpoint(response.final_url()) {
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

#[cfg(test)]
mod endpoint_contracts {
    use super::{official_record, verify_response};
    use crate::{
        LicenseErrorCode, LicenseRequest, LicenseResponse, LicenseTransport, LicenseTransportError,
        OfficialLicenseClient,
    };
    use ahcl_kit_config::AhclVersion;

    struct EchoEndpointTransport {
        expected_url: &'static str,
    }

    impl LicenseTransport for EchoEndpointTransport {
        fn execute(
            &self,
            request: &LicenseRequest,
        ) -> Result<LicenseResponse, LicenseTransportError> {
            assert_eq!(request.url(), self.expected_url);
            Ok(response(request.url()))
        }
    }

    fn response(final_url: &str) -> LicenseResponse {
        LicenseResponse::new(
            200,
            final_url.to_owned(),
            Some("application/json".to_owned()),
            false,
            b"not-json".to_vec(),
        )
    }

    #[test]
    fn normalized_official_endpoint_reaches_document_validation() {
        for (version, slug) in [
            (AhclVersion::V1_0, "ahcl-1-0"),
            (AhclVersion::V1_1, "ahcl-1-1"),
        ] {
            let record = official_record(version);
            let final_url = format!("https://AHCL.APERIP.COM:443/api/licenses/{slug}");
            let error = verify_response(&record, response(&final_url), 1_048_576)
                .expect_err("invalid body must reach JSON validation");

            assert_eq!(error.code(), LicenseErrorCode::JsonShape);
        }
    }

    #[test]
    fn client_requests_the_exact_endpoint_accepted_by_verification() {
        for (version, expected_url) in [
            (
                AhclVersion::V1_0,
                "https://ahcl.aperip.com/api/licenses/ahcl-1-0",
            ),
            (
                AhclVersion::V1_1,
                "https://ahcl.aperip.com/api/licenses/ahcl-1-1",
            ),
        ] {
            let client =
                OfficialLicenseClient::with_transport(EchoEndpointTransport { expected_url });
            let error = client
                .fetch(version)
                .expect_err("invalid body must reach JSON validation");

            assert_eq!(error.code(), LicenseErrorCode::JsonShape);
        }
    }

    #[test]
    fn same_origin_non_endpoint_urls_are_rejected_for_each_version() {
        for (version, slug, other_slug) in [
            (AhclVersion::V1_0, "ahcl-1-0", "ahcl-1-1"),
            (AhclVersion::V1_1, "ahcl-1-1", "ahcl-1-0"),
        ] {
            let record = official_record(version);
            let cases = [
                format!("https://ahcl.aperip.com/api/licenses/{other_slug}"),
                format!("https://ahcl.aperip.com/api/licenses/{slug}/"),
                format!("https://ahcl.aperip.com/api/licenses/{slug}?download=1"),
                format!("https://ahcl.aperip.com/api/licenses/{slug}?"),
                format!("https://ahcl.aperip.com/api/licenses/{slug}#record"),
                format!("https://ahcl.aperip.com/api/licenses/{slug}#"),
                format!("https://user@ahcl.aperip.com/api/licenses/{slug}"),
                format!("https://ahcl.aperip.com:444/api/licenses/{slug}"),
                format!("http://ahcl.aperip.com/api/licenses/{slug}"),
            ];

            for final_url in cases {
                let error = verify_response(&record, response(&final_url), 1_048_576)
                    .expect_err("non-endpoint URL must fail");
                assert_eq!(
                    error.code(),
                    LicenseErrorCode::RedirectOrigin,
                    "accepted non-endpoint URL: {final_url}"
                );
            }
        }
    }

    #[test]
    fn redirect_origin_error_is_stable_and_does_not_leak_the_url() {
        let record = official_record(AhclVersion::V1_1);
        let final_url = "https://ahcl.aperip.com/api/licenses/ahcl-1-1?secret=do-not-leak#private";
        let error = verify_response(&record, response(final_url), 1_048_576)
            .expect_err("query and fragment must fail endpoint validation");
        let rendered = error.to_string();

        assert_eq!(error.code(), LicenseErrorCode::RedirectOrigin);
        assert_eq!(
            rendered,
            "official license response redirected or used an unexpected origin"
        );
        assert!(!rendered.contains("do-not-leak"));
        assert!(!rendered.contains(final_url));
    }
}
