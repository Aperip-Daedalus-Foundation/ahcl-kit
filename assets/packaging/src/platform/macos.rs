// assets/packaging/src/platform/macos.rs - macOS Universal application packaging backend.
// Copyright (C) 2026 Aperip Daedalus Foundation. All rights reserved.
// SPDX-License-Identifier: LicenseRef-AHCL-1.1

use crate::command;
use crate::fsutil::{self, TempDir};
use crate::metadata::ProductMetadata;
use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const TARGETS: [&str; 2] = ["x86_64-apple-darwin", "aarch64-apple-darwin"];

pub fn build(root: &Path, product: &ProductMetadata, output: &Path) -> Result<(), Box<dyn Error>> {
    require_macos_host()?;
    // Both Darwin targets are built before lipo, so the PKG holds one Universal binary.
    compile_macos_targets(root, product)?;
    let work = TempDir::new("ahcl-kit-macos")?;
    let stage = work.path().join("root");
    stage_macos_bundle(root, product, &stage)?;
    let output_directory = fsutil::output_directory(root, output)?;
    let package = build_macos_pkg(product, &stage, &output_directory)?;
    println!("{}", package.display());
    Ok(())
}

fn require_macos_host() -> Result<(), Box<dyn Error>> {
    if std::env::consts::OS != "macos" {
        return Err(io::Error::other("macOS packages must be built on macOS").into());
    }
    Ok(())
}

fn compile_macos_targets(root: &Path, product: &ProductMetadata) -> Result<(), Box<dyn Error>> {
    for target in TARGETS {
        install_macos_target(target)?;
        build_macos_target(root, product, target)?;
    }
    Ok(())
}

fn install_macos_target(target: &str) -> Result<(), Box<dyn Error>> {
    command::run(
        Command::new("rustup").args(["target", "add", target]),
        "install macOS Rust target",
    )?;
    Ok(())
}

fn build_macos_target(
    root: &Path,
    product: &ProductMetadata,
    target: &str,
) -> Result<(), Box<dyn Error>> {
    command::run(
        Command::new("cargo")
            .current_dir(root)
            .args(["build", "--locked", "--release", "--target", target])
            .args(["--package", "ahcl-kit", "--bin"])
            .arg(&product.binary_name),
        "build macOS executable",
    )?;
    Ok(())
}

fn stage_macos_bundle(
    root: &Path,
    product: &ProductMetadata,
    stage: &Path,
) -> Result<(), Box<dyn Error>> {
    let application = stage
        .join("Applications")
        .join(format!("{}.app", product.product_name));
    prepare_macos_bundle(root, product, &application)?;
    link_macos_command(product, stage)
}

fn prepare_macos_bundle(
    root: &Path,
    product: &ProductMetadata,
    application: &Path,
) -> Result<(), Box<dyn Error>> {
    let contents = application.join("Contents");
    let executable = contents.join("MacOS").join(&product.binary_name);
    let resources = contents.join("Resources");
    create_macos_directories(&executable, &resources)?;
    create_universal_executable(root, product, &executable)?;
    copy_macos_resources(root, &resources)?;
    write_macos_plist(root, product, &contents)
}

fn create_macos_directories(executable: &Path, resources: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir_all(executable.parent().expect("executable has parent"))?;
    std::fs::create_dir_all(resources)?;
    Ok(())
}

fn create_universal_executable(
    root: &Path,
    product: &ProductMetadata,
    executable: &Path,
) -> Result<(), Box<dyn Error>> {
    let x64 = built_binary(root, TARGETS[0], &product.binary_name);
    let arm64 = built_binary(root, TARGETS[1], &product.binary_name);
    command::run(
        Command::new("lipo")
            .arg("-create")
            .arg(x64)
            .arg(arm64)
            .arg("-output")
            .arg(executable),
        "create Universal 2 executable",
    )?;
    fsutil::set_executable(executable)?;
    command::run(
        Command::new("lipo")
            .arg(executable)
            .arg("-verify_arch")
            .args(["x86_64", "arm64"]),
        "verify Universal 2 executable",
    )?;
    Ok(())
}

fn copy_macos_resources(root: &Path, resources: &Path) -> Result<(), Box<dyn Error>> {
    fsutil::copy_file(
        &root.join("assets/branding/ahcl-kit.icns"),
        &resources.join("ahcl-kit.icns"),
    )?;
    fsutil::copy_file(&root.join("LICENSE"), &resources.join("LICENSE"))?;
    fsutil::copy_tree(&root.join(".ahcl"), &resources.join("AHCL"))?;
    Ok(())
}

fn write_macos_plist(
    root: &Path,
    product: &ProductMetadata,
    contents: &Path,
) -> Result<(), Box<dyn Error>> {
    fsutil::render_template(
        &root.join("assets/packaging/macos/Info.plist.template"),
        &contents.join("Info.plist"),
        &macos_plist_values(product),
    )?;
    command::run(
        Command::new("plutil")
            .arg("-lint")
            .arg(contents.join("Info.plist")),
        "validate macOS Info.plist",
    )?;
    Ok(())
}

fn macos_plist_values(product: &ProductMetadata) -> BTreeMap<&str, String> {
    let mut values = BTreeMap::new();
    values.insert("@PRODUCT_NAME@", product.product_name.clone());
    values.insert("@BINARY_NAME@", product.binary_name.clone());
    values.insert("@IDENTIFIER@", product.identifier.clone());
    values.insert("@VERSION@", product.version.clone());
    values.insert("@COPYRIGHT@", product.copyright.clone());
    values.insert("@DESCRIPTION@", product.description.clone());
    values.insert("@HOMEPAGE@", product.homepage.clone());
    values.insert("@LANGUAGE@", product.language.clone());
    values
}

fn link_macos_command(product: &ProductMetadata, stage: &Path) -> Result<(), Box<dyn Error>> {
    let command_link = stage.join("usr/local/bin").join(&product.binary_name);
    fsutil::create_symlink(
        &PathBuf::from(format!(
            "/Applications/{}.app/Contents/MacOS/{}",
            product.product_name, product.binary_name
        )),
        &command_link,
    )?;
    Ok(())
}

fn build_macos_pkg(
    product: &ProductMetadata,
    stage: &Path,
    output_directory: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    let package = output_directory.join(format!(
        "ahcl-kit-{}-macos-universal-apple-darwin.pkg",
        product.version
    ));
    command::run(
        Command::new("pkgbuild")
            .arg("--root")
            .arg(stage)
            .arg("--identifier")
            .arg(&product.identifier)
            .arg("--version")
            .arg(&product.version)
            .arg("--install-location")
            .arg("/")
            .arg(&package),
        "build macOS PKG",
    )?;
    Ok(package)
}

fn built_binary(root: &Path, target: &str, binary_name: &str) -> PathBuf {
    root.join("target")
        .join(target)
        .join("release")
        .join(binary_name)
}
