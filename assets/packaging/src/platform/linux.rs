// assets/packaging/src/platform/linux.rs - Linux DEB and RPM packaging backend.
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

pub fn build(
    root: &Path,
    product: &ProductMetadata,
    target: &str,
    deb_architecture: &str,
    rpm_architecture: &str,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    if std::env::consts::OS != "linux" {
        return Err(io::Error::other("Linux packages must be built on Linux").into());
    }
    match (target, deb_architecture, rpm_architecture) {
        ("x86_64-unknown-linux-gnu", "amd64", "x86_64")
        | ("aarch64-unknown-linux-gnu", "arm64", "aarch64") => {}
        _ => return Err(io::Error::other("unsupported Linux architecture mapping").into()),
    }

    command::run(
        Command::new("rustup").args(["target", "add", target]),
        "install Linux Rust target",
    )?;
    command::run(
        Command::new("cargo")
            .current_dir(root)
            .args(["build", "--locked", "--release", "--target", target])
            .args(["--package", "ahcl-kit-cli", "--bin"])
            .arg(&product.binary_name),
        "build Linux executable",
    )?;

    let work = TempDir::new("ahcl-kit-linux")?;
    let payload = work.path().join("payload");
    stage_payload(root, product, target, &payload)?;

    let output_directory = fsutil::output_directory(root, output)?;
    let deb = build_deb(
        work.path(),
        &payload,
        product,
        deb_architecture,
        &output_directory,
    )?;
    let rpm = build_rpm(
        work.path(),
        &payload,
        product,
        rpm_architecture,
        &output_directory,
    )?;

    println!("{}", deb.display());
    println!("{}", rpm.display());
    Ok(())
}

fn stage_payload(
    root: &Path,
    product: &ProductMetadata,
    target: &str,
    payload: &Path,
) -> Result<(), Box<dyn Error>> {
    let installed_binary = payload.join("usr/lib/ahcl-kit").join(&product.binary_name);
    fsutil::copy_file(
        &root
            .join("target")
            .join(target)
            .join("release")
            .join(&product.binary_name),
        &installed_binary,
    )?;
    fsutil::set_executable(&installed_binary)?;
    fsutil::create_symlink(
        &PathBuf::from(format!("/usr/lib/ahcl-kit/{}", product.binary_name)),
        &payload.join("usr/bin").join(&product.binary_name),
    )?;

    fsutil::copy_file(
        &root.join("assets/branding/ahcl-kit.svg"),
        &payload.join("usr/share/icons/hicolor/scalable/apps/ahcl-kit.svg"),
    )?;
    for size in [16, 32, 48, 128, 256, 512] {
        fsutil::copy_file(
            &root.join(format!("assets/branding/ahcl-kit-{size}.png")),
            &payload.join(format!(
                "usr/share/icons/hicolor/{size}x{size}/apps/ahcl-kit.png"
            )),
        )?;
    }
    fsutil::copy_file(
        &root.join("LICENSE"),
        &payload.join("usr/share/doc/ahcl-kit/LICENSE"),
    )?;
    fsutil::copy_tree(
        &root.join(".ahcl"),
        &payload.join("usr/share/doc/ahcl-kit/AHCL"),
    )?;

    let release_date = git_release_date(root)?;
    let mut values = BTreeMap::new();
    values.insert("@IDENTIFIER@", product.identifier.clone());
    values.insert("@PRODUCT_NAME@", product.product_name.clone());
    values.insert("@DESCRIPTION@", product.description.clone());
    values.insert("@PUBLISHER@", product.publisher.clone());
    values.insert("@BINARY_NAME@", product.binary_name.clone());
    values.insert("@HOMEPAGE@", product.homepage.clone());
    values.insert("@VERSION@", product.version.clone());
    values.insert("@RELEASE_DATE@", release_date);
    fsutil::render_template(
        &root.join("assets/packaging/linux/com.aperip.ahcl-kit.metainfo.xml.template"),
        &payload.join(format!(
            "usr/share/metainfo/{}.metainfo.xml",
            product.identifier
        )),
        &values,
    )?;
    Ok(())
}

fn build_deb(
    work: &Path,
    payload: &Path,
    product: &ProductMetadata,
    architecture: &str,
    output: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    let root = work.join("deb");
    fsutil::copy_tree(payload, &root)?;
    let control = format!(
        "Package: ahcl-kit\nVersion: {}\nSection: devel\nPriority: optional\nArchitecture: {}\nMaintainer: {}\nHomepage: {}\nDescription: {}\n AHCL Kit initializes and generates AHCL project materials from configured adapters.\n",
        product.version, architecture, product.publisher, product.homepage, product.description
    );
    fsutil::write_text(&root.join("DEBIAN/control"), &control)?;
    let package = output.join(format!(
        "ahcl-kit-{}-linux-{architecture}.deb",
        product.version
    ));
    command::run(
        Command::new("dpkg-deb")
            .args(["--build", "--root-owner-group"])
            .arg(&root)
            .arg(&package),
        "build DEB package",
    )?;
    Ok(package)
}

fn build_rpm(
    work: &Path,
    payload: &Path,
    product: &ProductMetadata,
    architecture: &str,
    output: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    let top = work.join("rpmbuild");
    for directory in ["BUILD", "BUILDROOT", "RPMS", "SOURCES", "SPECS", "SRPMS"] {
        std::fs::create_dir_all(top.join(directory))?;
    }
    fsutil::copy_tree(payload, &top.join("SOURCES/payload"))?;
    let spec = format!(
        "%global debug_package %{{nil}}\nName: ahcl-kit\nVersion: {}\nRelease: 1%{{?dist}}\nSummary: {}\nLicense: LicenseRef-AHCL-1.1\nURL: {}\nBuildArch: {}\n\n%description\n{}.\n\n%install\nmkdir -p %{{buildroot}}\ncp -a %{{_sourcedir}}/payload/. %{{buildroot}}/\n\n%files\n/usr/bin/{}\n/usr/lib/ahcl-kit/{}\n/usr/share/icons/hicolor/*/apps/ahcl-kit.*\n/usr/share/metainfo/{}.metainfo.xml\n/usr/share/doc/ahcl-kit\n",
        product.version,
        product.description,
        product.homepage,
        architecture,
        product.description,
        product.binary_name,
        product.binary_name,
        product.identifier
    );
    let spec_path = top.join("SPECS/ahcl-kit.spec");
    fsutil::write_text(&spec_path, &spec)?;
    command::run(
        Command::new("rpmbuild")
            .arg("--define")
            .arg(format!("_topdir {}", top.display()))
            .args(["-bb"])
            .arg(&spec_path),
        "build RPM package",
    )?;
    let built = fsutil::find_file(&top.join("RPMS"), "rpm")?;
    let package = output.join(format!(
        "ahcl-kit-{}-linux-{architecture}.rpm",
        product.version
    ));
    fsutil::copy_file(&built, &package)?;
    Ok(package)
}

fn git_release_date(root: &Path) -> Result<String, Box<dyn Error>> {
    let output = command::capture(
        Command::new("git")
            .current_dir(root)
            .args(["log", "-1", "--format=%cs"]),
        "read source date",
    )?;
    let date = String::from_utf8(output.stdout)?.trim().to_owned();
    let valid = date.len() == 10
        && date.bytes().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7) && byte == b'-'
                || !matches!(index, 4 | 7) && byte.is_ascii_digit()
        });
    if !valid {
        return Err(io::Error::other("git returned an invalid release date").into());
    }
    Ok(date)
}
