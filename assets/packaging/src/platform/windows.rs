// assets/packaging/src/platform/windows.rs - Windows MSI packaging backend.
// Copyright (C) 2026 Aperip Daedalus Foundation. All rights reserved.
// SPDX-License-Identifier: LicenseRef-AHCL-1.1

use crate::command;
use crate::fsutil::{self, TempDir};
use crate::metadata::ProductMetadata;
use std::error::Error;
use std::io;
use std::path::Path;
use std::process::Command;

pub fn build(
    root: &Path,
    product: &ProductMetadata,
    architecture: &str,
    target: &str,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    if std::env::consts::OS != "windows" {
        return Err(io::Error::other("Windows packages must be built on Windows").into());
    }
    match (architecture, target) {
        ("x64", "x86_64-pc-windows-msvc") | ("arm64", "aarch64-pc-windows-msvc") => {}
        _ => return Err(io::Error::other("unsupported Windows architecture mapping").into()),
    }

    command::run(
        Command::new("rustup").args(["target", "add", target]),
        "install Windows Rust target",
    )?;
    command::run(
        Command::new("cargo")
            .current_dir(root)
            .args(["build", "--locked", "--release", "--target", target])
            .args(["--package", "ahcl-kit-cli", "--bin"])
            .arg(&product.binary_name),
        "build Windows executable",
    )?;

    let binary = root
        .join("target")
        .join(target)
        .join("release")
        .join(format!("{}.exe", product.binary_name));
    if !binary.is_file() {
        return Err(
            io::Error::other(format!("built executable is missing: {}", binary.display())).into(),
        );
    }

    let work = TempDir::new("ahcl-kit-windows")?;
    let materials_archive = work.path().join("AHCL-MATERIALS.zip");
    command::run(
        Command::new("tar")
            .current_dir(root)
            .args(["-a", "-c", "-f"])
            .arg(&materials_archive)
            .arg(".ahcl"),
        "archive AHCL materials",
    )?;

    let output_directory = fsutil::output_directory(root, output)?;
    let package = output_directory.join(format!(
        "ahcl-kit-{}-windows-{architecture}.msi",
        product.version
    ));
    let definition = root.join("assets/packaging/windows/ahcl-kit.wxs");
    let icon = root.join("assets/branding/ahcl-kit.ico");
    let license = root.join("LICENSE");
    let wix = std::env::var_os("AHCL_WIX").unwrap_or_else(|| "wix".into());

    command::run(
        Command::new(wix)
            .arg("build")
            .arg(definition)
            .args(["-arch", architecture])
            .args(["-d", &format!("ProductName={}", product.product_name)])
            .args(["-d", &format!("Publisher={}", product.publisher)])
            .args(["-d", &format!("ProductVersion={}", product.version)])
            .args(["-d", &format!("Description={}", product.description)])
            .args(["-d", &format!("Homepage={}", product.homepage)])
            .args(["-d", &format!("BinaryPath={}", binary.display())])
            .args(["-d", &format!("LicensePath={}", license.display())])
            .args([
                "-d",
                &format!("MaterialsArchivePath={}", materials_archive.display()),
            ])
            .args(["-d", &format!("IconPath={}", icon.display())])
            .arg("-out")
            .arg(&package),
        "build Windows MSI",
    )?;

    println!("{}", package.display());
    Ok(())
}
