// assets/packaging/src/command.rs - Checked external command execution.
// Copyright (C) 2026 Aperip Daedalus Foundation. All rights reserved.
// SPDX-License-Identifier: LicenseRef-AHCL-1.1

use std::error::Error;
use std::io;
use std::process::{Command, Output};

pub fn run(command: &mut Command, operation: &str) -> Result<(), Box<dyn Error>> {
    eprintln!("+ {command:?}");
    let status = command.status()?;
    if !status.success() {
        return Err(io::Error::other(format!("{operation} failed with status {status}")).into());
    }
    Ok(())
}

pub fn capture(command: &mut Command, operation: &str) -> Result<Output, Box<dyn Error>> {
    let output = command.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "{operation} failed with status {}: {}",
            output.status,
            stderr.trim()
        ))
        .into());
    }
    Ok(output)
}
