// assets/packaging/src/metadata.rs - Resolve product metadata from Cargo.
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
    let document = cargo_metadata_document(root)?;
    require_neutral_language(metadata_from_document(&document)?)
}

fn cargo_metadata_document(root: &Path) -> Result<Value, Box<dyn Error>> {
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
    Ok(serde_json::from_slice(&output.stdout)?)
}

// Both the ahcl-kit package record and the workspace product table are required.
fn metadata_from_document(document: &Value) -> Result<ProductMetadata, Box<dyn Error>> {
    let package = ahcl_kit_package(document)?;
    let product = product_metadata_table(document)?;
    let (version, description, homepage) = package_fields(package)?;
    let identity = product_fields(product)?;
    Ok(ProductMetadata {
        version,
        description,
        homepage,
        product_name: identity.product_name,
        binary_name: identity.binary_name,
        publisher: identity.publisher,
        copyright: identity.copyright,
        identifier: identity.identifier,
        language: identity.language,
    })
}

fn ahcl_kit_package(document: &Value) -> Result<&Value, Box<dyn Error>> {
    document["packages"]
        .as_array()
        .and_then(|packages| {
            packages
                .iter()
                .find(|package| package["name"].as_str() == Some("ahcl-kit"))
        })
        .ok_or_else(|| io::Error::other("cargo metadata did not return ahcl-kit").into())
}

fn product_metadata_table(
    document: &Value,
) -> Result<&serde_json::Map<String, Value>, Box<dyn Error>> {
    document["metadata"]["ahcl-kit"]
        .as_object()
        .ok_or_else(|| io::Error::other("missing workspace.metadata.ahcl-kit").into())
}

fn package_fields(package: &Value) -> Result<(String, String, String), Box<dyn Error>> {
    Ok((
        required(package, "version")?,
        required(package, "description")?,
        required(package, "homepage")?,
    ))
}

fn product_fields(
    product: &serde_json::Map<String, Value>,
) -> Result<ProductIdentity, Box<dyn Error>> {
    Ok(ProductIdentity {
        product_name: required_object(product, "product-name")?,
        binary_name: required_object(product, "binary-name")?,
        publisher: required_object(product, "publisher")?,
        copyright: required_object(product, "copyright")?,
        identifier: required_object(product, "identifier")?,
        language: required_object(product, "language")?,
    })
}

// Named so the workspace product record is not a six-string tuple.
struct ProductIdentity {
    product_name: String,
    binary_name: String,
    publisher: String,
    copyright: String,
    identifier: String,
    language: String,
}

fn require_neutral_language(metadata: ProductMetadata) -> Result<ProductMetadata, Box<dyn Error>> {
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
