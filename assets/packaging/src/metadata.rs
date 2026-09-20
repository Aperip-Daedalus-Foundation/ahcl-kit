// assets/packaging/src/metadata.rs - Resolve product metadata from Cargo.
// Copyright (C) 2026 Aperip Daedalus Foundation. All rights reserved.
// SPDX-License-Identifier: LicenseRef-AHCL-1.1

use crate::command;
use serde_json::{Value, json};
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug)]
pub struct ProductMetadata {
    pub version: String,
    pub description: String,
    pub homepage: String,
    pub product_name: String,
    pub binary_name: String,
    pub publisher: String,
    pub copyright: String,
    pub identifier: String,
    pub language: String,
}

impl ProductMetadata {
    pub fn to_pretty_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&json!({
            "version": self.version,
            "description": self.description,
            "homepage": self.homepage,
            "product_name": self.product_name,
            "binary_name": self.binary_name,
            "publisher": self.publisher,
            "copyright": self.copyright,
            "identifier": self.identifier,
            "language": self.language,
        }))
    }
}

pub fn repository_root() -> Result<PathBuf, Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    Ok(portable_absolute_path(root))
}

#[cfg(windows)]
fn portable_absolute_path(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    text.strip_prefix(r"\\?\")
        .map_or(path.clone(), PathBuf::from)
}

#[cfg(not(windows))]
fn portable_absolute_path(path: PathBuf) -> PathBuf {
    path
}

pub fn load(root: &Path) -> Result<ProductMetadata, Box<dyn Error>> {
    let output = command::capture(
        Command::new("cargo")
            .arg("metadata")
            .arg("--format-version")
            .arg("1")
            .arg("--no-deps")
            .arg("--manifest-path")
            .arg(root.join("Cargo.toml")),
        "cargo metadata",
    )?;
    let document: Value = serde_json::from_slice(&output.stdout)?;
    let package = document["packages"]
        .as_array()
        .and_then(|packages| {
            packages
                .iter()
                .find(|package| package["name"].as_str() == Some("ahcl-kit-cli"))
        })
        .ok_or_else(|| io::Error::other("cargo metadata did not return ahcl-kit-cli"))?;
    let product = document["metadata"]["ahcl-kit"]
        .as_object()
        .ok_or_else(|| io::Error::other("missing workspace.metadata.ahcl-kit"))?;

    let metadata = ProductMetadata {
        version: required(package, "version")?,
        description: required(package, "description")?,
        homepage: required(package, "homepage")?,
        product_name: required_object(product, "product-name")?,
        binary_name: required_object(product, "binary-name")?,
        publisher: required_object(product, "publisher")?,
        copyright: required_object(product, "copyright")?,
        identifier: required_object(product, "identifier")?,
        language: required_object(product, "language")?,
    };
    if metadata.language != "neutral" {
        return Err(io::Error::other("product language must be neutral").into());
    }
    Ok(metadata)
}

fn required(value: &Value, key: &str) -> Result<String, Box<dyn Error>> {
    value[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::other(format!("missing metadata field: {key}")).into())
}

fn required_object(
    value: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, Box<dyn Error>> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::other(format!("missing product metadata field: {key}")).into())
}
