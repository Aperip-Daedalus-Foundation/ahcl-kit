use ahcl_kit_cli::{
    InvocationError, InvocationRegistry, OutputFormat, not_wired_report, render_json, render_text,
};
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let initial_cwd = match std::env::current_dir() {
        Ok(path) => path,
        Err(_) => {
            return write_error(
                "cli.initial_cwd",
                "initial current directory is unavailable",
            );
        }
    };
    let argv = std::env::args_os().collect::<Vec<OsString>>();
    let invocation = match InvocationRegistry::installed().parse_from(argv, initial_cwd) {
        Ok(invocation) => invocation,
        Err(InvocationError::Arguments(error)) => {
            let code = if error.use_stderr() { 1 } else { 0 };
            let _ = error.print();
            return ExitCode::from(code);
        }
        Err(error) => return write_error(error.code(), &error.to_string()),
    };
    let report = not_wired_report(&invocation);
    let rendered = match invocation.output_format() {
        OutputFormat::Text => render_text(&report),
        OutputFormat::Json => match render_json(&report) {
            Ok(rendered) => rendered,
            Err(_) => return write_error("cli.output", "output serialization failed"),
        },
    };
    if std::io::stdout().write_all(rendered.as_bytes()).is_err() {
        return ExitCode::from(1);
    }
    ExitCode::from(report.exit_code())
}

fn write_error(code: &str, message: &str) -> ExitCode {
    let rendered = format!("[error] {code}: {message}\n");
    let _ = std::io::stderr().write_all(rendered.as_bytes());
    ExitCode::from(1)
}
