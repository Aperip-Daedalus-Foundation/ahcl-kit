// crates/ahcl-kit/src/main.rs - Composition root for the AHCL Kit executable.
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

use ahcl_kit_cli::{
    CommandReport, ConcreteRuntime, InvocationError, InvocationRegistry, OutputFormat,
    ParsedInvocation, render_json, render_text, run, system_utc_date,
};
use ahcl_kit_config::{LanguageContributor, LanguageHost, LanguageInstallation};
use ahcl_kit_core::{CommandId, UtcDate};
use std::io::Write;
use std::process::ExitCode;

static CONTRIBUTORS: [&dyn LanguageContributor; 2] = [
    &ahcl_kit_cargo::CONTRIBUTOR,
    &ahcl_kit_javascript::CONTRIBUTOR,
];
static HOSTS: [&dyn LanguageHost; 2] = [&ahcl_kit_cargo::HOST, &ahcl_kit_javascript::HOST];

struct Startup {
    invocation: ParsedInvocation,
    current_date: Option<UtcDate>,
}

fn main() -> ExitCode {
    // The composition root only sequences invocation, the installed language
    // registry, and output. Ecosystem behavior stays in the registered hosts.
    match startup() {
        Ok(startup) => render_report(&startup.invocation, startup.current_date),
        Err(code) => code,
    }
}

fn startup() -> Result<Startup, ExitCode> {
    let initial_cwd = std::env::current_dir().map_err(|_| {
        write_error(
            "cli.initial_cwd",
            "initial current directory is unavailable",
        )
    })?;
    let invocation = InvocationRegistry::installed()
        .parse_from(std::env::args_os(), initial_cwd)
        .map_err(exit_for_invocation)?;
    let current_date = invocation_date(&invocation)?;
    Ok(Startup {
        invocation,
        current_date,
    })
}

fn exit_for_invocation(error: InvocationError) -> ExitCode {
    match error {
        InvocationError::Arguments(error) => {
            let code = if error.use_stderr() { 1 } else { 0 };
            let _ = error.print();
            ExitCode::from(code)
        }
        error => write_error(error.code(), &error.to_string()),
    }
}

fn invocation_date(invocation: &ParsedInvocation) -> Result<Option<UtcDate>, ExitCode> {
    if invocation.command_id() != CommandId::ProjectInit {
        return Ok(None);
    }
    match system_utc_date() {
        Ok(date) => Ok(Some(date)),
        Err(error) => Err(write_error(error.code(), &error.to_string())),
    }
}

fn render_report(invocation: &ParsedInvocation, current_date: Option<UtcDate>) -> ExitCode {
    let installed = LanguageInstallation::new(&CONTRIBUTORS, &HOSTS);
    let report = run(
        invocation,
        &mut ConcreteRuntime::new(current_date, installed),
    );
    let Ok(rendered) = render_output(invocation, &report) else {
        return write_error("cli.output", "output serialization failed");
    };
    if std::io::stdout().write_all(rendered.as_bytes()).is_err() {
        return ExitCode::from(1);
    }
    ExitCode::from(report.exit_code())
}

fn render_output(invocation: &ParsedInvocation, report: &CommandReport) -> Result<String, ()> {
    match invocation.output_format() {
        OutputFormat::Text => Ok(render_text(report)),
        OutputFormat::Json => render_json(report).map_err(|_| ()),
    }
}

fn write_error(code: &str, message: &str) -> ExitCode {
    let rendered = format!("[error] {code}: {message}\n");
    let _ = std::io::stderr().write_all(rendered.as_bytes());
    ExitCode::from(1)
}
