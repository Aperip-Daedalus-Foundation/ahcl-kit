// crates/ahcl-kit-cli/build.rs - Platform executable metadata generation.
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

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let root = repository_root();
    let manifest_path = root.join("Cargo.toml");
    let icon_path = root.join("assets/branding/ahcl-kit.ico");
    println!("cargo:rerun-if-changed={}", manifest_path.display());
    println!("cargo:rerun-if-changed={}", icon_path.display());

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest = fs::read_to_string(&manifest_path).expect("root Cargo.toml must be readable");
    let document = manifest
        .parse::<toml::Value>()
        .expect("root Cargo.toml must be valid TOML");
    let package = table(&document, &["workspace", "package"]);
    let product = table(&document, &["workspace", "metadata", "ahcl-kit"]);

    let product_name = string(product, "product-name");
    let binary_name = string(product, "binary-name");
    let publisher = string(product, "publisher");
    let copyright = string(product, "copyright");
    let homepage = string(package, "homepage");

    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(icon_path.to_str().expect("icon path must be UTF-8"))
        .set_language(0)
        .set("ProductName", product_name)
        .set("FileDescription", product_name)
        .set("CompanyName", publisher)
        .set("LegalCopyright", copyright)
        .set("InternalName", binary_name)
        .set("OriginalFilename", &format!("{binary_name}.exe"))
        .set("Comments", homepage)
        .set_manifest(
            r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
    </windowsSettings>
  </application>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security>
  </trustInfo>
</assembly>"#,
        )
        .compile()
        .expect("Windows executable resources must compile");
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn table<'a>(document: &'a toml::Value, path: &[&str]) -> &'a toml::value::Table {
    let mut current = document;
    for segment in path {
        current = current
            .get(*segment)
            .unwrap_or_else(|| panic!("missing Cargo.toml table segment: {segment}"));
    }
    current
        .as_table()
        .expect("Cargo.toml value must be a table")
}

fn string<'a>(table: &'a toml::value::Table, key: &str) -> &'a str {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("missing Cargo.toml string: {key}"))
}
