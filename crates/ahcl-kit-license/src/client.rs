// crates/ahcl-kit-license/src/client.rs - Official AHCL license download client.
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

use crate::model::{LicenseError, LicenseErrorCode, LicenseRequest, LicenseResponse};
use crate::verify::{official_record, verify_response};
use crate::{LicenseTransportError, VerifiedLicense};
use ahcl_kit_config::AhclVersion;
use std::time::Duration;
use ureq::ResponseExt;

pub trait LicenseTransport: Send + Sync {
    fn execute(&self, request: &LicenseRequest) -> Result<LicenseResponse, LicenseTransportError>;
}

pub struct OfficialLicenseClient {
    transport: Box<dyn LicenseTransport>,
}

impl OfficialLicenseClient {
    pub fn new() -> Self {
        Self::with_transport(UreqLicenseTransport::new())
    }

    pub fn with_transport<T>(transport: T) -> Self
    where
        T: LicenseTransport + 'static,
    {
        Self {
            transport: Box::new(transport),
        }
    }

    pub fn fetch(&self, version: AhclVersion) -> Result<VerifiedLicense, LicenseError> {
        let record = official_record(version);
        let request = LicenseRequest::official(record.endpoint_url().to_string());
        let response = self
            .transport
            .execute(&request)
            .map_err(map_transport_error)?;
        verify_response(&record, response, request.response_limit())
    }
}

impl Default for OfficialLicenseClient {
    fn default() -> Self {
        Self::new()
    }
}

fn map_transport_error(error: LicenseTransportError) -> LicenseError {
    let code = match error {
        LicenseTransportError::RequestFailed => LicenseErrorCode::Transport,
        LicenseTransportError::ResponseTooLarge => LicenseErrorCode::ResponseSize,
    };
    LicenseError::new(code)
}

struct UreqLicenseTransport {
    agent: ureq::Agent,
}

impl UreqLicenseTransport {
    fn new() -> Self {
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

impl LicenseTransport for UreqLicenseTransport {
    fn execute(&self, request: &LicenseRequest) -> Result<LicenseResponse, LicenseTransportError> {
        let mut response = self
            .agent
            .get(request.url())
            .header("Accept", request.accept())
            .call()
            .map_err(|_| LicenseTransportError::RequestFailed)?;
        let status = response.status().as_u16();
        let final_url = response.get_uri().to_string();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let read_limit = request.response_limit().saturating_add(1);
        let body = response
            .body_mut()
            .with_config()
            .limit(read_limit as u64)
            .read_to_vec()
            .map_err(map_body_error)?;
        if body.len() > request.response_limit() {
            return Err(LicenseTransportError::ResponseTooLarge);
        }

        Ok(LicenseResponse::new(
            status,
            final_url,
            content_type,
            false,
            body,
        ))
    }
}

fn map_body_error(error: ureq::Error) -> LicenseTransportError {
    match error {
        ureq::Error::BodyExceedsLimit(_) => LicenseTransportError::ResponseTooLarge,
        _ => LicenseTransportError::RequestFailed,
    }
}
