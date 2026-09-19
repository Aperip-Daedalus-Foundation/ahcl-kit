use ahcl_kit_config::{AhclVersion, EffectiveConfig, ProjectIdentity};
use ahcl_kit_core::{ChangePlan, ProjectRoot, ResolvedGraph, UtcDate};
use ahcl_kit_license::VerifiedLicense;
use std::error::Error;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdapterKind {
    Cargo,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanScope {
    ConfigInit,
    ProjectInit,
    ProjectFiles,
    License,
    Dependencies,
    ThirdParty,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedAdapter {
    adapter: AdapterKind,
    graph: ResolvedGraph,
}

impl ResolvedAdapter {
    pub fn new(adapter: AdapterKind, graph: ResolvedGraph) -> Self {
        Self { adapter, graph }
    }

    pub fn adapter(&self) -> AdapterKind {
        self.adapter
    }

    pub fn graph(&self) -> &ResolvedGraph {
        &self.graph
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlanRequest<'a> {
    scopes: &'a [PlanScope],
    config: Option<&'a EffectiveConfig>,
    license: Option<&'a VerifiedLicense>,
    adapters: &'a [ResolvedAdapter],
    current_date: Option<UtcDate>,
    force: bool,
}

impl<'a> PlanRequest<'a> {
    pub(crate) fn new(
        scopes: &'a [PlanScope],
        config: Option<&'a EffectiveConfig>,
        license: Option<&'a VerifiedLicense>,
        adapters: &'a [ResolvedAdapter],
        current_date: Option<UtcDate>,
        force: bool,
    ) -> Self {
        Self {
            scopes,
            config,
            license,
            adapters,
            current_date,
            force,
        }
    }

    pub fn scopes(&self) -> &[PlanScope] {
        self.scopes
    }

    pub fn config(&self) -> Option<&EffectiveConfig> {
        self.config
    }

    pub fn license(&self) -> Option<&VerifiedLicense> {
        self.license
    }

    pub fn adapters(&self) -> &[ResolvedAdapter] {
        self.adapters
    }

    pub fn current_date(&self) -> Option<UtcDate> {
        self.current_date
    }

    pub fn force(&self) -> bool {
        self.force
    }
}

pub trait CommandRuntime {
    fn load_config(&mut self, project: &ProjectRoot) -> Result<EffectiveConfig, RuntimeError>;

    fn prepare_project_init(
        &mut self,
        project: &ProjectRoot,
        identity: Option<&ProjectIdentity>,
        current_date: UtcDate,
    ) -> Result<EffectiveConfig, RuntimeError>;

    fn current_utc_date(&mut self) -> Result<UtcDate, RuntimeError>;

    fn fetch_official_license(
        &mut self,
        version: AhclVersion,
    ) -> Result<VerifiedLicense, RuntimeError>;

    fn resolve_adapter(
        &mut self,
        adapter: AdapterKind,
        project: &ProjectRoot,
        config: &EffectiveConfig,
    ) -> Result<ResolvedGraph, RuntimeError>;

    fn plan(
        &mut self,
        project: &ProjectRoot,
        request: PlanRequest<'_>,
    ) -> Result<ChangePlan, RuntimeError>;

    fn compare(
        &mut self,
        project: &ProjectRoot,
        plan: &ChangePlan,
    ) -> Result<ChangePlan, RuntimeError>;

    fn apply(&mut self, project: &ProjectRoot, plan: &ChangePlan) -> Result<(), RuntimeError>;
}

pub struct RuntimeError {
    code: String,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl fmt::Debug for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeError")
            .field("code", &self.code)
            .field("message", &"runtime operation failed")
            .finish()
    }
}

impl RuntimeError {
    pub fn operation(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            source: None,
        }
    }

    pub fn with_source(
        code: impl Into<String>,
        source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            code: code.into(),
            source: Some(Box::new(source)),
        }
    }

    pub fn code(&self) -> &str {
        &self.code
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("runtime operation failed")
    }
}

impl Error for RuntimeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct UnavailableRuntime;

impl CommandRuntime for UnavailableRuntime {
    fn load_config(&mut self, _: &ProjectRoot) -> Result<EffectiveConfig, RuntimeError> {
        Err(unavailable())
    }

    fn prepare_project_init(
        &mut self,
        _: &ProjectRoot,
        _: Option<&ProjectIdentity>,
        _: UtcDate,
    ) -> Result<EffectiveConfig, RuntimeError> {
        Err(unavailable())
    }

    fn current_utc_date(&mut self) -> Result<UtcDate, RuntimeError> {
        Err(unavailable())
    }

    fn fetch_official_license(&mut self, _: AhclVersion) -> Result<VerifiedLicense, RuntimeError> {
        Err(unavailable())
    }

    fn resolve_adapter(
        &mut self,
        _: AdapterKind,
        _: &ProjectRoot,
        _: &EffectiveConfig,
    ) -> Result<ResolvedGraph, RuntimeError> {
        Err(unavailable())
    }

    fn plan(&mut self, _: &ProjectRoot, _: PlanRequest<'_>) -> Result<ChangePlan, RuntimeError> {
        Err(unavailable())
    }

    fn compare(&mut self, _: &ProjectRoot, _: &ChangePlan) -> Result<ChangePlan, RuntimeError> {
        Err(unavailable())
    }

    fn apply(&mut self, _: &ProjectRoot, _: &ChangePlan) -> Result<(), RuntimeError> {
        Err(unavailable())
    }
}

fn unavailable() -> RuntimeError {
    RuntimeError::operation("cli.runtime_unavailable")
}
