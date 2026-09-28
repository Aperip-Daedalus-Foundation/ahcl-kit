// assets/packaging/src/main.rs - Native distribution packaging entry point.
//
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

mod command;
mod fsutil;
mod metadata;
mod platform;

use clap::{Parser, Subcommand};
use metadata::ProductMetadata;
use std::error::Error;
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(name = "ahcl-dist", version, about)]
struct Cli {
    #[command(subcommand)]
    command: DistributionCommand,
}

#[derive(Debug, Subcommand)]
enum DistributionCommand {
    /// Print resolved product metadata as JSON.
    Metadata,
    /// Build a Windows MSI for one native architecture.
    Windows {
        #[arg(long, value_parser = ["x64", "arm64"])]
        architecture: String,
        #[arg(long, value_parser = ["x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc"])]
        target: String,
        #[arg(long, default_value = "dist")]
        output: PathBuf,
    },
    /// Build a Universal 2 macOS application bundle and PKG.
    Macos {
        #[arg(long, default_value = "dist")]
        output: PathBuf,
    },
    /// Build DEB and RPM packages for one native Linux architecture.
    Linux {
        #[arg(long, value_parser = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"])]
        target: String,
        #[arg(long, value_parser = ["amd64", "arm64"])]
        deb_architecture: String,
        #[arg(long, value_parser = ["x86_64", "aarch64"])]
        rpm_architecture: String,
        #[arg(long, default_value = "dist")]
        output: PathBuf,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ahcl-dist: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let root = metadata::repository_root()?;
    let product = metadata::load(&root)?;
    dispatch(&root, &product, cli.command)
}

fn dispatch(
    root: &Path,
    product: &ProductMetadata,
    command: DistributionCommand,
) -> Result<(), Box<dyn Error>> {
    match command {
        DistributionCommand::Metadata => print_metadata(product),
        DistributionCommand::Windows {
            architecture,
            target,
            output,
        } => platform::windows::build(root, product, &architecture, &target, &output),
        DistributionCommand::Macos { output } => platform::macos::build(root, product, &output),
        DistributionCommand::Linux {
            target,
            deb_architecture,
            rpm_architecture,
            output,
        } => platform::linux::build(
            root,
            product,
            &target,
            &deb_architecture,
            &rpm_architecture,
            &output,
        ),
    }
}

fn print_metadata(product: &ProductMetadata) -> Result<(), Box<dyn Error>> {
    println!("{}", product.to_pretty_json()?);
    Ok(())
}
