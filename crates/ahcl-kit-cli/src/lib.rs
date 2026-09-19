//! Command parsing, project discovery, batch isolation, and output foundations.

mod args;
mod batch;
mod discovery;
mod dispatch;
mod output;
mod runtime;
mod runtime_impl;

pub use args::{
    InvocationError, InvocationRegistry, OutputFormat, ParsedInvocation, ProjectIdentityArgs,
};
pub use batch::{BatchExecution, BatchItem, BatchStatus, BatchValue, execute_batch};
pub use discovery::{DiscoveryError, DiscoveryMode};
pub use dispatch::run;
pub use output::{
    AggregateCounts, CommandReport, OutputChange, OutputChangeKind, OutputDiagnostic,
    OutputSeverity, ProjectReport, ProjectStatus, not_wired_report, render_json, render_text,
};
pub use runtime::{
    AdapterKind, CommandRuntime, PlanRequest, PlanScope, ResolvedAdapter, RuntimeError,
    RuntimePlan, UnavailableRuntime,
};
pub use runtime_impl::{ConcreteRuntime, system_utc_date};
