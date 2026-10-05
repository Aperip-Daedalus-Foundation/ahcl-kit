// crates/ahcl-kit-cli/src/args.rs - Command-line parsing and invocation registry.
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

use crate::discovery::{self, DiscoveryError, DiscoveryMode};
use ahcl_kit_core::CommandId;
use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

macro_rules! copyright_notice {
    () => {
        concat!(
            "Copyright (C) 2026 Aperip Daedalus Foundation. All rights reserved.\n",
            env!("CARGO_PKG_REPOSITORY")
        )
    };
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Clone, Debug, Args)]
struct ProjectArgs {
    #[arg(value_name = "PROJECT")]
    projects: Vec<PathBuf>,

    #[arg(long, value_name = "FILE")]
    projects_from: Option<PathBuf>,

    #[arg(long)]
    fail_fast: bool,

    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
}

#[derive(Clone, Copy, Debug, Default, Args)]
struct WriteArgs {
    #[arg(long)]
    dry_run: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProjectIdentityArgs {
    pub name: Option<String>,
    pub repository: Option<String>,
    pub right_holders: Vec<String>,
}

#[derive(Clone, Debug, Args)]
struct ConfigInitArgs {
    #[command(flatten)]
    project: ProjectArgs,
    #[command(flatten)]
    write: WriteArgs,
    #[arg(long)]
    force: bool,
}

#[derive(Clone, Debug, Args)]
struct ConfigValidateArgs {
    #[command(flatten)]
    project: ProjectArgs,
}

#[derive(Clone, Debug, Args)]
struct ConfigShowArgs {
    #[command(flatten)]
    project: ProjectArgs,
    #[arg(long, action = ArgAction::SetTrue, required = true)]
    resolved: bool,
}

#[derive(Clone, Debug, Args)]
struct ProjectInitArgs {
    #[command(flatten)]
    project: ProjectArgs,
    #[command(flatten)]
    write: WriteArgs,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    repository: Option<String>,
    #[arg(long = "right-holder")]
    right_holders: Vec<String>,
}

#[derive(Clone, Debug, Args)]
struct ProjectWriteArgs {
    #[command(flatten)]
    project: ProjectArgs,
    #[command(flatten)]
    write: WriteArgs,
}

#[derive(Clone, Debug, Args)]
struct ProjectReadArgs {
    #[command(flatten)]
    project: ProjectArgs,
}

#[derive(Clone, Debug, Parser)]
#[command(
    name = "ahcl",
    version = concat!(env!("CARGO_PKG_VERSION"), "\n\n", copyright_notice!()),
    about = env!("CARGO_PKG_DESCRIPTION"),
    after_help = copyright_notice!(),
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: TopCommand,
}

#[derive(Clone, Debug, Subcommand)]
enum TopCommand {
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    License {
        #[command(subcommand)]
        command: LicenseCommand,
    },
    Dependency {
        #[command(subcommand)]
        command: DependencyCommand,
    },
    ThirdParty {
        #[command(subcommand)]
        command: ThirdPartyCommand,
    },
    /// Print the absolute path of this executable.
    Where,
}

#[derive(Clone, Debug, Subcommand)]
enum ConfigCommand {
    Init(ConfigInitArgs),
    Validate(ConfigValidateArgs),
    Show(ConfigShowArgs),
}

#[derive(Clone, Debug, Subcommand)]
enum ProjectCommand {
    Init(ProjectInitArgs),
    Generate(ProjectWriteArgs),
    Check(ProjectReadArgs),
}

#[derive(Clone, Debug, Subcommand)]
enum LicenseCommand {
    Sync(ProjectWriteArgs),
}

#[derive(Clone, Debug, Subcommand)]
enum DependencyCommand {
    Generate(ProjectWriteArgs),
}

#[derive(Clone, Debug, Subcommand)]
enum ThirdPartyCommand {
    Generate(ProjectWriteArgs),
}

#[derive(Clone, Debug)]
enum ProjectCommandTree {
    Config(ConfigCommand),
    Project(ProjectCommand),
    License(LicenseCommand),
    Dependency(DependencyCommand),
    ThirdParty(ThirdPartyCommand),
}

#[derive(Clone, Debug)]
pub enum ParsedRequest {
    Invocation(ParsedInvocation),
    Where,
}

#[derive(Clone, Copy, Debug)]
pub struct InvocationRegistry {
    canonical_name: &'static str,
    aliases: &'static [&'static str],
}

impl Default for InvocationRegistry {
    fn default() -> Self {
        Self::installed()
    }
}

impl InvocationRegistry {
    pub const fn new(canonical_name: &'static str, aliases: &'static [&'static str]) -> Self {
        Self {
            canonical_name,
            aliases,
        }
    }

    pub const fn installed() -> Self {
        Self::new("ahcl", &[])
    }

    pub fn parse_from<I, T>(
        &self,
        argv: I,
        initial_cwd: PathBuf,
    ) -> Result<ParsedRequest, InvocationError>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let argv = self.invocation_argv(argv, &initial_cwd)?;
        let invocation_name = self.canonical_name.to_owned();
        let cli = Cli::try_parse_from(argv).map_err(InvocationError::Arguments)?;
        let Some(command) = project_command(cli.command) else {
            return Ok(ParsedRequest::Where);
        };
        Ok(ParsedRequest::Invocation(ParsedInvocation {
            invocation_name,
            initial_cwd,
            command,
        }))
    }

    fn invocation_argv<I, T>(
        &self,
        argv: I,
        initial_cwd: &Path,
    ) -> Result<Vec<OsString>, InvocationError>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        absolute_initial_cwd(initial_cwd)?;
        let mut argv = argv.into_iter().map(Into::into).collect::<Vec<_>>();
        self.accept_argv0(&mut argv)?;
        Ok(argv)
    }

    fn accept_argv0(&self, argv: &mut [OsString]) -> Result<(), InvocationError> {
        let Some(observed) = argv.first() else {
            return Err(InvocationError::MissingArgv0);
        };
        let (observed, normalized) = normalize_invocation(observed)?;
        if !self.accepts(&normalized) {
            return Err(InvocationError::UnknownInvocation { observed });
        }
        if let Some(program) = argv.first_mut() {
            *program = OsString::from(self.canonical_name);
        }
        Ok(())
    }

    fn accepts(&self, invocation_name: &str) -> bool {
        self.canonical_name.eq_ignore_ascii_case(invocation_name)
            || self
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(invocation_name))
    }
}

#[derive(Clone, Debug)]
pub struct ParsedInvocation {
    invocation_name: String,
    initial_cwd: PathBuf,
    command: ProjectCommandTree,
}

impl ParsedInvocation {
    pub fn invocation_name(&self) -> &str {
        &self.invocation_name
    }

    pub fn initial_cwd(&self) -> &Path {
        &self.initial_cwd
    }

    pub fn command_id(&self) -> CommandId {
        match &self.command {
            ProjectCommandTree::Config(command) => config_command_id(command),
            ProjectCommandTree::Project(command) => project_command_id(command),
            ProjectCommandTree::License(_) => CommandId::LicenseSync,
            ProjectCommandTree::Dependency(_) => CommandId::DependencyGenerate,
            ProjectCommandTree::ThirdParty(_) => CommandId::ThirdPartyGenerate,
        }
    }

    pub fn projects(&self) -> &[PathBuf] {
        &self.project_args().projects
    }

    pub fn projects_from(&self) -> Option<&Path> {
        self.project_args().projects_from.as_deref()
    }

    pub fn fail_fast(&self) -> bool {
        self.project_args().fail_fast
    }

    pub fn output_format(&self) -> OutputFormat {
        self.project_args().format
    }

    pub fn dry_run(&self) -> bool {
        match &self.command {
            ProjectCommandTree::Config(ConfigCommand::Init(args)) => args.write.dry_run,
            ProjectCommandTree::Project(ProjectCommand::Init(args)) => args.write.dry_run,
            ProjectCommandTree::Project(ProjectCommand::Generate(args))
            | ProjectCommandTree::License(LicenseCommand::Sync(args))
            | ProjectCommandTree::Dependency(DependencyCommand::Generate(args))
            | ProjectCommandTree::ThirdParty(ThirdPartyCommand::Generate(args)) => {
                args.write.dry_run
            }
            ProjectCommandTree::Config(_) | ProjectCommandTree::Project(_) => false,
        }
    }

    pub fn force(&self) -> bool {
        matches!(
            &self.command,
            ProjectCommandTree::Config(ConfigCommand::Init(ConfigInitArgs { force: true, .. }))
        )
    }

    pub fn project_identity(&self) -> Option<ProjectIdentityArgs> {
        match &self.command {
            ProjectCommandTree::Project(ProjectCommand::Init(args)) => Some(ProjectIdentityArgs {
                name: args.name.clone(),
                repository: args.repository.clone(),
                right_holders: args.right_holders.clone(),
            }),
            _ => None,
        }
    }

    pub fn discovery_mode(&self) -> DiscoveryMode {
        match self.command_id() {
            CommandId::ConfigInit | CommandId::ProjectInit => DiscoveryMode::Initialization,
            _ => DiscoveryMode::ExistingConfig,
        }
    }

    pub fn resolve_projects(&self) -> Result<Vec<PathBuf>, DiscoveryError> {
        discovery::resolve_projects(
            &self.initial_cwd,
            self.projects(),
            self.projects_from(),
            self.discovery_mode(),
        )
    }

    fn project_args(&self) -> &ProjectArgs {
        match &self.command {
            ProjectCommandTree::Config(command) => config_project_args(command),
            ProjectCommandTree::Project(command) => project_command_args(command),
            ProjectCommandTree::License(LicenseCommand::Sync(args))
            | ProjectCommandTree::Dependency(DependencyCommand::Generate(args))
            | ProjectCommandTree::ThirdParty(ThirdPartyCommand::Generate(args)) => &args.project,
        }
    }
}

fn absolute_initial_cwd(initial_cwd: &Path) -> Result<(), InvocationError> {
    if initial_cwd.is_absolute() {
        Ok(())
    } else {
        Err(InvocationError::InitialCwdNotAbsolute)
    }
}

fn project_command(command: TopCommand) -> Option<ProjectCommandTree> {
    match command {
        TopCommand::Where => None,
        TopCommand::Config { command } => Some(ProjectCommandTree::Config(command)),
        TopCommand::Project { command } => Some(ProjectCommandTree::Project(command)),
        TopCommand::License { command } => Some(ProjectCommandTree::License(command)),
        TopCommand::Dependency { command } => Some(ProjectCommandTree::Dependency(command)),
        TopCommand::ThirdParty { command } => Some(ProjectCommandTree::ThirdParty(command)),
    }
}

pub fn executable_location() -> Result<PathBuf, InvocationError> {
    let current = std::env::current_exe().map_err(|_| InvocationError::ExecutableUnavailable)?;
    absolute_executable(current)
}

fn absolute_executable(path: PathBuf) -> Result<PathBuf, InvocationError> {
    if let Ok(canonical) = fs::canonicalize(&path) {
        return Ok(plain_absolute(canonical));
    }
    if path.is_absolute() {
        return Ok(path);
    }
    Err(InvocationError::ExecutableUnavailable)
}

fn plain_absolute(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{unc}"));
    }
    if let Some(disk) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(disk);
    }
    path
}

fn config_command_id(command: &ConfigCommand) -> CommandId {
    match command {
        ConfigCommand::Init(_) => CommandId::ConfigInit,
        ConfigCommand::Validate(_) => CommandId::ConfigValidate,
        ConfigCommand::Show(_) => CommandId::ConfigShowResolved,
    }
}

fn project_command_id(command: &ProjectCommand) -> CommandId {
    match command {
        ProjectCommand::Init(_) => CommandId::ProjectInit,
        ProjectCommand::Generate(_) => CommandId::ProjectGenerate,
        ProjectCommand::Check(_) => CommandId::ProjectCheck,
    }
}

fn config_project_args(command: &ConfigCommand) -> &ProjectArgs {
    match command {
        ConfigCommand::Init(args) => &args.project,
        ConfigCommand::Validate(args) => &args.project,
        ConfigCommand::Show(args) => &args.project,
    }
}

fn project_command_args(command: &ProjectCommand) -> &ProjectArgs {
    match command {
        ProjectCommand::Init(args) => &args.project,
        ProjectCommand::Generate(args) => &args.project,
        ProjectCommand::Check(args) => &args.project,
    }
}

#[derive(Debug)]
pub enum InvocationError {
    MissingArgv0,
    UnknownInvocation { observed: String },
    InitialCwdNotAbsolute,
    ExecutableUnavailable,
    Arguments(clap::Error),
}

impl InvocationError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MissingArgv0 => "cli.missing_argv0",
            Self::UnknownInvocation { .. } => "cli.unknown_invocation",
            Self::InitialCwdNotAbsolute => "cli.initial_cwd",
            Self::ExecutableUnavailable => "cli.executable",
            Self::Arguments(_) => "cli.arguments",
        }
    }
}

impl fmt::Display for InvocationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingArgv0 => formatter.write_str("invocation name is missing"),
            Self::UnknownInvocation { observed } => {
                write!(formatter, "unknown invocation name: {observed}")
            }
            Self::InitialCwdNotAbsolute => {
                formatter.write_str("initial current directory must be absolute")
            }
            Self::ExecutableUnavailable => formatter.write_str("executable path is unavailable"),
            Self::Arguments(error) => error.fmt(formatter),
        }
    }
}

impl Error for InvocationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Arguments(error) => Some(error),
            _ => None,
        }
    }
}

fn normalize_invocation(value: &OsStr) -> Result<(String, String), InvocationError> {
    let observed = value.to_string_lossy();
    let basename = observed
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
        .ok_or(InvocationError::MissingArgv0)?;
    let normalized = if basename
        .get(basename.len().saturating_sub(4)..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".exe"))
    {
        &basename[..basename.len() - 4]
    } else {
        basename
    };
    Ok((basename.to_owned(), normalized.to_owned()))
}
