// assets/packaging/src/main.rs - Native distribution packaging entry point.
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

mod command;
mod fsutil;
mod metadata;
mod platform;

use clap::{Parser, Subcommand};
use std::error::Error;
use std::path::PathBuf;

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

    match cli.command {
        DistributionCommand::Metadata => println!("{}", product.to_pretty_json()?),
        DistributionCommand::Windows {
            architecture,
            target,
            output,
        } => platform::windows::build(&root, &product, &architecture, &target, &output)?,
        DistributionCommand::Macos { output } => platform::macos::build(&root, &product, &output)?,
        DistributionCommand::Linux {
            target,
            deb_architecture,
            rpm_architecture,
            output,
        } => platform::linux::build(
            &root,
            &product,
            &target,
            &deb_architecture,
            &rpm_architecture,
            &output,
        )?,
    }
    Ok(())
}
