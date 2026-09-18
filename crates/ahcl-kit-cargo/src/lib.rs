//! Cargo dependency resolution and byte-preserving license evidence collection.

mod adapter;
mod collector;
mod graph;
mod limits;

pub use adapter::{CargoAdapter, CargoError, CargoResolveRequest};
pub use limits::{EvidenceLimits, PackageDirectoryInput, assign_package_directories};
