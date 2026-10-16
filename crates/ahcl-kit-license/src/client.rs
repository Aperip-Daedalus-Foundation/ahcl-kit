// crates/ahcl-kit-license/src/client.rs - Official AHCL license download client.
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

use crate::model::{
    LicenseError, LicenseErrorCode, LicenseRequest, LicenseResponse, OFFICIAL_CONNECT_TIMEOUT,
    OFFICIAL_TOTAL_TIMEOUT,
};
use crate::verify::{official_record, verify_response};
use crate::{LicenseTransportError, VerifiedLicense};
use ahcl_kit_config::AhclVersion;
use ureq::ResponseExt;

const OFFICIAL_RETRIES: usize = 9;

pub trait LicenseTransport: Send + Sync {
    fn execute(&self, request: &LicenseRequest) -> Result<LicenseResponse, LicenseTransportError>;
}

pub struct OfficialLicenseClient {
    transport: Box<dyn LicenseTransport>,
    retry_reporter: Option<Box<dyn Fn(usize, usize) + Send + Sync>>,
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
            retry_reporter: None,
        }
    }

    pub fn with_retry_reporter<F>(mut self, reporter: F) -> Self
    where
        F: Fn(usize, usize) + Send + Sync + 'static,
    {
        self.retry_reporter = Some(Box::new(reporter));
        self
    }

    pub fn fetch(&self, version: AhclVersion) -> Result<VerifiedLicense, LicenseError> {
        let record = official_record(version);
        let request = LicenseRequest::official(record.endpoint_url().to_string());
        let response = self.execute(&request).map_err(map_transport_error)?;
        verify_response(&record, response, request.response_limit())
    }

    fn execute(&self, request: &LicenseRequest) -> Result<LicenseResponse, LicenseTransportError> {
        let mut retries_completed = 0;
        loop {
            let result = self.transport.execute(request);
            if !should_retry(&result, retries_completed) {
                return result;
            }
            retries_completed += 1;
            self.report_retry(retries_completed);
        }
    }

    fn report_retry(&self, retry: usize) {
        if let Some(reporter) = &self.retry_reporter {
            reporter(retry, OFFICIAL_RETRIES);
        }
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
        LicenseTransportError::ConnectTimeout => LicenseErrorCode::ConnectTimeout,
        LicenseTransportError::RequestTimeout => LicenseErrorCode::Timeout,
        LicenseTransportError::TlsFailed => LicenseErrorCode::Tls,
        LicenseTransportError::ResponseReadFailed => LicenseErrorCode::ResponseRead,
        LicenseTransportError::ResponseTooLarge => LicenseErrorCode::ResponseSize,
    };
    LicenseError::new(code)
}

fn retryable(error: LicenseTransportError) -> bool {
    matches!(
        error,
        LicenseTransportError::RequestFailed
            | LicenseTransportError::ConnectTimeout
            | LicenseTransportError::RequestTimeout
            | LicenseTransportError::ResponseReadFailed
    )
}

fn retryable_status(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 500..=599)
}

fn should_retry(
    result: &Result<LicenseResponse, LicenseTransportError>,
    retries_completed: usize,
) -> bool {
    if retries_completed >= OFFICIAL_RETRIES {
        return false;
    }
    match result {
        Ok(response) => retryable_status(response.status()),
        Err(error) => retryable(*error),
    }
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
            .accept_encoding("gzip")
            .timeout_connect(Some(OFFICIAL_CONNECT_TIMEOUT))
            .timeout_global(Some(OFFICIAL_TOTAL_TIMEOUT))
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
            .map_err(map_call_error)?;
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

fn map_call_error(error: ureq::Error) -> LicenseTransportError {
    match error {
        ureq::Error::Timeout(ureq::Timeout::Resolve | ureq::Timeout::Connect) => {
            LicenseTransportError::ConnectTimeout
        }
        ureq::Error::Timeout(_) => LicenseTransportError::RequestTimeout,
        ureq::Error::Tls(_) | ureq::Error::Pem(_) | ureq::Error::Rustls(_) => {
            LicenseTransportError::TlsFailed
        }
        _ => LicenseTransportError::RequestFailed,
    }
}

fn map_body_error(error: ureq::Error) -> LicenseTransportError {
    match error {
        ureq::Error::BodyExceedsLimit(_) => LicenseTransportError::ResponseTooLarge,
        ureq::Error::Timeout(_) => LicenseTransportError::RequestTimeout,
        ureq::Error::Tls(_) | ureq::Error::Pem(_) | ureq::Error::Rustls(_) => {
            LicenseTransportError::TlsFailed
        }
        _ => LicenseTransportError::ResponseReadFailed,
    }
}
