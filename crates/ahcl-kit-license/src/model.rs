use ahcl_kit_config::AhclVersion;
use std::fmt;
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LicenseRequest {
    url: String,
    accept: &'static str,
    allow_redirects: bool,
    connect_timeout: Duration,
    total_timeout: Duration,
    response_limit: usize,
}

impl LicenseRequest {
    pub(crate) fn official(url: String) -> Self {
        Self {
            url,
            accept: "application/json",
            allow_redirects: false,
            connect_timeout: Duration::from_secs(5),
            total_timeout: Duration::from_secs(30),
            response_limit: 1_048_576,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn accept(&self) -> &str {
        self.accept
    }

    pub fn allow_redirects(&self) -> bool {
        self.allow_redirects
    }

    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    pub fn total_timeout(&self) -> Duration {
        self.total_timeout
    }

    pub fn response_limit(&self) -> usize {
        self.response_limit
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LicenseResponse {
    status: u16,
    final_url: String,
    content_type: Option<String>,
    redirected: bool,
    body: Vec<u8>,
}

impl LicenseResponse {
    pub fn new(
        status: u16,
        final_url: String,
        content_type: Option<String>,
        redirected: bool,
        body: Vec<u8>,
    ) -> Self {
        Self {
            status,
            final_url,
            content_type,
            redirected,
            body,
        }
    }

    pub fn set_status(&mut self, status: u16) {
        self.status = status;
    }

    pub fn set_final_url(&mut self, final_url: String) {
        self.final_url = final_url;
    }

    pub fn set_content_type(&mut self, content_type: Option<String>) {
        self.content_type = content_type;
    }

    pub fn set_redirected(&mut self, redirected: bool) {
        self.redirected = redirected;
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn final_url(&self) -> &str {
        &self.final_url
    }

    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    pub fn redirected(&self) -> bool {
        self.redirected
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LicenseTransportError {
    RequestFailed,
    ResponseTooLarge,
}

impl fmt::Display for LicenseTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestFailed => formatter.write_str("official license transport failed"),
            Self::ResponseTooLarge => {
                formatter.write_str("official license response exceeded the size limit")
            }
        }
    }
}

impl std::error::Error for LicenseTransportError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LicenseErrorCode {
    Transport,
    RedirectOrigin,
    Status,
    ContentType,
    ResponseSize,
    JsonShape,
    MetadataMismatch,
    DigestMismatch,
}

impl LicenseErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transport => "license.transport",
            Self::RedirectOrigin => "license.redirect_origin",
            Self::Status => "license.status",
            Self::ContentType => "license.content_type",
            Self::ResponseSize => "license.response_size",
            Self::JsonShape => "license.json_shape",
            Self::MetadataMismatch => "license.metadata_mismatch",
            Self::DigestMismatch => "license.digest_mismatch",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LicenseError {
    code: LicenseErrorCode,
}

impl LicenseError {
    pub(crate) fn new(code: LicenseErrorCode) -> Self {
        Self { code }
    }

    pub fn code(&self) -> LicenseErrorCode {
        self.code
    }
}

impl fmt::Display for LicenseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self.code {
            LicenseErrorCode::Transport => "official license transport failed",
            LicenseErrorCode::RedirectOrigin => {
                "official license response redirected or used an unexpected origin"
            }
            LicenseErrorCode::Status => "official license response returned an unexpected status",
            LicenseErrorCode::ContentType => {
                "official license response had an invalid content type"
            }
            LicenseErrorCode::ResponseSize => "official license response exceeded the size limit",
            LicenseErrorCode::JsonShape => "official license response was not valid JSON",
            LicenseErrorCode::MetadataMismatch => {
                "official license response metadata did not match"
            }
            LicenseErrorCode::DigestMismatch => "official license response digest did not match",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for LicenseError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedLicense {
    pub version: AhclVersion,
    pub slug: String,
    pub title: String,
    pub source_filename: String,
    pub body: String,
    pub sha256: String,
}
