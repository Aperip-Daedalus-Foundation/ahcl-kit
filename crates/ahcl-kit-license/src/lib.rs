//! Constrained download and verification of official AHCL license records.

mod client;
mod model;
mod verify;

pub use client::{LicenseTransport, OfficialLicenseClient};
pub use model::{
    LicenseError, LicenseErrorCode, LicenseRequest, LicenseResponse, LicenseTransportError,
    VerifiedLicense,
};
