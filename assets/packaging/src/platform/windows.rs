// assets/packaging/src/platform/windows.rs - Windows MSI packaging backend.
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
use crate::fsutil::{self, TempDir};
use crate::metadata::ProductMetadata;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn build(
    root: &Path,
    product: &ProductMetadata,
    architecture: &str,
    target: &str,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    require_windows_host()?;
    require_windows_mapping(architecture, target)?;
    compile_windows(root, product, target)?;
    package_windows(root, product, architecture, target, output)
}

fn require_windows_host() -> Result<(), Box<dyn Error>> {
    if std::env::consts::OS != "windows" {
        return Err(io::Error::other("Windows packages must be built on Windows").into());
    }
    Ok(())
}

// Only these architecture and Rust target pairs are accepted.
fn require_windows_mapping(architecture: &str, target: &str) -> Result<(), Box<dyn Error>> {
    match (architecture, target) {
        ("x64", "x86_64-pc-windows-msvc") | ("arm64", "aarch64-pc-windows-msvc") => Ok(()),
        _ => Err(io::Error::other("unsupported Windows architecture mapping").into()),
    }
}

fn compile_windows(
    root: &Path,
    product: &ProductMetadata,
    target: &str,
) -> Result<(), Box<dyn Error>> {
    command::run(
        Command::new("rustup").args(["target", "add", target]),
        "install Windows Rust target",
    )?;
    let resources = TempDir::new("ahcl-kit-windows-resources")?;
    let resource = super::windows_resources::compile_resources(root, product, resources.path())?;
    link_windows_executable(root, product, target, &resource)?;
    require_embedded_metadata(
        &windows_release_binary(root, target, &product.binary_name),
        product,
        root,
    )
}

// Extra rustc arguments are ignored when Cargo considers the package fresh.
fn link_windows_executable(
    root: &Path,
    product: &ProductMetadata,
    target: &str,
    resource: &Path,
) -> Result<(), Box<dyn Error>> {
    command::run(
        Command::new("cargo").current_dir(root).args([
            "clean",
            "--release",
            "--target",
            target,
            "--package",
            "ahcl-kit",
        ]),
        "reset Windows executable link",
    )?;
    command::run(
        Command::new("cargo")
            .current_dir(root)
            .args(["rustc", "--locked", "--release", "--target", target])
            .args(["--package", "ahcl-kit", "--bin"])
            .arg(&product.binary_name)
            .arg("--")
            .arg("-C")
            .arg(format!("link-arg={}", resource.display())),
        "build Windows executable",
    )
}

fn require_embedded_metadata(
    binary: &Path,
    product: &ProductMetadata,
    root: &Path,
) -> Result<(), Box<dyn Error>> {
    let bytes = fs::read(binary)?;
    require_utf16(&bytes, &product.product_name, "product name")?;
    require_utf16(&bytes, &product.copyright, "copyright")?;
    require_utf16(&bytes, &product.version, "version")?;
    require_utf16(
        &bytes,
        &format!("{}.exe", product.binary_name),
        "original filename",
    )?;
    require_bytes(&bytes, b"longPathAware", "long path manifest")?;
    let icon = root.join("assets/branding/ahcl-kit.ico");
    require_bytes(
        &bytes,
        &super::windows_resources::icon_sample(&icon)?,
        "icon",
    )
}

fn require_utf16(bytes: &[u8], text: &str, label: &str) -> Result<(), Box<dyn Error>> {
    let encoded = text
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    require_bytes(bytes, &encoded, label)
}

fn require_bytes(bytes: &[u8], needle: &[u8], label: &str) -> Result<(), Box<dyn Error>> {
    if !needle.is_empty() && bytes.windows(needle.len()).any(|window| window == needle) {
        return Ok(());
    }
    Err(io::Error::other(format!("Windows executable is missing {label} metadata")).into())
}

fn package_windows(
    root: &Path,
    product: &ProductMetadata,
    architecture: &str,
    target: &str,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    let binary = windows_release_binary(root, target, &product.binary_name);
    require_built_binary(&binary)?;
    let work = TempDir::new("ahcl-kit-windows")?;
    let materials_archive = archive_ahcl_materials(root, work.path())?;
    let output_directory = fsutil::output_directory(root, output)?;
    build_windows_package(
        root,
        product,
        architecture,
        &binary,
        &materials_archive,
        &output_directory,
    )
}

fn windows_release_binary(root: &Path, target: &str, binary_name: &str) -> PathBuf {
    root.join("target")
        .join(target)
        .join("release")
        .join(format!("{binary_name}.exe"))
}

fn require_built_binary(binary: &Path) -> Result<(), Box<dyn Error>> {
    if !binary.is_file() {
        return Err(
            io::Error::other(format!("built executable is missing: {}", binary.display())).into(),
        );
    }
    Ok(())
}

fn archive_ahcl_materials(root: &Path, work: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let materials_archive = work.join("AHCL-MATERIALS.zip");
    command::run(
        Command::new("tar")
            .current_dir(root)
            .args(["-a", "-c", "-f"])
            .arg(&materials_archive)
            .arg(".ahcl"),
        "archive AHCL materials",
    )?;
    Ok(materials_archive)
}

fn build_windows_package(
    root: &Path,
    product: &ProductMetadata,
    architecture: &str,
    binary: &Path,
    materials_archive: &Path,
    output_directory: &Path,
) -> Result<(), Box<dyn Error>> {
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
