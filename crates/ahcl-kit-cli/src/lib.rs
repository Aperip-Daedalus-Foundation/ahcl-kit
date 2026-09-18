//! Command parsing, project discovery, batch isolation, and output foundations.

mod args;
mod batch;
mod discovery;
mod output;

pub use args::{
    InvocationError, InvocationRegistry, OutputFormat, ParsedInvocation, ProjectIdentityArgs,
};
pub use batch::{BatchExecution, BatchItem, BatchStatus, BatchValue, execute_batch};
pub use discovery::{DiscoveryError, DiscoveryMode};
pub use output::{
    AggregateCounts, CommandReport, OutputChange, OutputChangeKind, OutputDiagnostic,
    OutputSeverity, ProjectReport, ProjectStatus, not_wired_report, render_json, render_text,
};
