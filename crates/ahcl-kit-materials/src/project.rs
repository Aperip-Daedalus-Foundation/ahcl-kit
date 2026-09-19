use crate::LayoutPolicy;
use crate::render;
use ahcl_kit_config::{
    AhclVersion, ConfigDocument, ConfigError, ConfigSkeleton, EffectiveConfig, ProjectIdentity,
    ScalarValue,
};
use ahcl_kit_core::{ChangePlan, PlanError, ProjectEntry, ProjectView, RepoPath, UtcDate};
use ahcl_kit_license::VerifiedLicense;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaterialsErrorCode {
    IdentityRequired,
    ConfigExists,
    InvalidConfig,
    InvalidLayout,
    LicenseVersionMismatch,
    StateInvalid,
    LinkOrReparsePoint,
    ManagedTreeInvalid,
    Plan,
    View,
    Filesystem(&'static str),
}

impl MaterialsErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IdentityRequired => "config.identity_required",
            Self::ConfigExists => "config.exists",
            Self::InvalidConfig => "config.invalid",
            Self::InvalidLayout => "materials.layout_invalid",
            Self::LicenseVersionMismatch => "license.version_mismatch",
            Self::StateInvalid => "materials.state_invalid",
            Self::LinkOrReparsePoint => "materials.link_or_reparse",
            Self::ManagedTreeInvalid => "materials.managed_tree_invalid",
            Self::Plan => "materials.plan",
            Self::View => "materials.view",
            Self::Filesystem(code) => code,
        }
    }
}

impl PartialEq<&str> for MaterialsErrorCode {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl fmt::Display for MaterialsErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterialsError {
    code: MaterialsErrorCode,
    message: &'static str,
    path: Option<RepoPath>,
}

impl MaterialsError {
    pub(crate) fn new(code: MaterialsErrorCode) -> Self {
        Self {
            message: default_message(code),
            code,
            path: None,
        }
    }

    pub fn code(&self) -> MaterialsErrorCode {
        self.code
    }

    pub fn message(&self) -> &'static str {
        self.message
    }

    pub fn path(&self) -> Option<&RepoPath> {
        self.path.as_ref()
    }

    pub(crate) fn filesystem(code: &'static str, message: &'static str) -> Self {
        Self {
            code: MaterialsErrorCode::Filesystem(code),
            message,
            path: None,
        }
    }

    pub(crate) fn filesystem_at(
        code: &'static str,
        message: &'static str,
        path: RepoPath,
    ) -> Self {
        Self {
            code: MaterialsErrorCode::Filesystem(code),
            message,
            path: Some(path),
        }
    }

    pub(crate) fn root(code: &'static str, message: &'static str) -> Self {
        Self::filesystem(code, message)
    }

    pub(crate) fn at_path(
        code: &'static str,
        message: &'static str,
        path: &crate::SafeRelPath,
    ) -> Self {
        Self::filesystem_at(code, message, path.repo_path().clone())
    }

    pub(crate) fn from_plan(error: PlanError) -> Self {
        map_plan_error(error)
    }
}

impl fmt::Display for MaterialsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(path) => write!(formatter, "{}: {path}", self.message),
            None => formatter.write_str(self.message),
        }
    }
}

fn default_message(code: MaterialsErrorCode) -> &'static str {
    match code {
        MaterialsErrorCode::IdentityRequired => {
            "project identity is required before AHCL initialization"
        }
        MaterialsErrorCode::ConfigExists => {
            "existing configuration cannot be replaced without force"
        }
        MaterialsErrorCode::InvalidConfig => "project configuration is invalid",
        MaterialsErrorCode::InvalidLayout => "AHCL materials layout is invalid",
        MaterialsErrorCode::LicenseVersionMismatch => {
            "verified license version does not match project configuration"
        }
        MaterialsErrorCode::StateInvalid => "managed third-party state is invalid",
        MaterialsErrorCode::LinkOrReparsePoint => {
            "managed third-party tree contains a link or reparse point"
        }
        MaterialsErrorCode::ManagedTreeInvalid => "managed third-party tree is invalid",
        MaterialsErrorCode::Plan => "AHCL material plan could not be constructed",
        MaterialsErrorCode::View => "project entries could not be observed",
        MaterialsErrorCode::Filesystem(_) => "filesystem operation failed",
    }
}

impl std::error::Error for MaterialsError {}

pub struct ProjectMaterialGenerator;

impl ProjectMaterialGenerator {
    pub fn plan_init(
        view: &dyn ProjectView,
        identity: Option<&ProjectIdentity>,
        license: &VerifiedLicense,
        current_date: UtcDate,
        force: bool,
    ) -> Result<ChangePlan, MaterialsError> {
        let config_path = repo_path(".ahclkitconfigs")?;
        let entry = view
            .entry(&config_path)
            .map_err(|_| MaterialsError::new(MaterialsErrorCode::View))?;

        let (document, write_config) = match entry {
            ProjectEntry::Absent => {
                let identity = identity
                    .filter(|identity| complete_identity(identity))
                    .ok_or_else(|| MaterialsError::new(MaterialsErrorCode::IdentityRequired))?;
                (
                    render_config(identity, license.version, current_date)?,
                    true,
                )
            }
            ProjectEntry::File(_) if identity.is_some() && !force => {
                return Err(MaterialsError::new(MaterialsErrorCode::ConfigExists));
            }
            ProjectEntry::File(_) if identity.is_some() => {
                let identity = identity
                    .filter(|identity| complete_identity(identity))
                    .ok_or_else(|| MaterialsError::new(MaterialsErrorCode::IdentityRequired))?;
                (
                    render_config(identity, license.version, current_date)?,
                    true,
                )
            }
            ProjectEntry::File(bytes) => {
                let mut document = ConfigDocument::parse_bytes(&bytes).map_err(map_config_error)?;
                let config = EffectiveConfig::resolve(&document).map_err(map_config_error)?;
                if !complete_config_identity(&config) {
                    return Err(MaterialsError::new(MaterialsErrorCode::IdentityRequired));
                }
                let write_config = config.project().adoption_date().is_none();
                if write_config {
                    document
                        .upsert_scalar(
                            Some("project"),
                            "adoption-date",
                            ScalarValue::String(current_date.to_string()),
                        )
                        .map_err(map_config_error)?;
                }
                (document, write_config)
            }
            ProjectEntry::Other => {
                return Err(MaterialsError::new(MaterialsErrorCode::InvalidConfig));
            }
        };

        let config = EffectiveConfig::resolve(&document).map_err(map_config_error)?;
        validate_license(&config, license)?;
        let adoption_date = config.project().adoption_date().unwrap_or(current_date);
        let mut desired = ChangePlan::new();
        if write_config {
            write(&mut desired, config_path, document.render().into_bytes())?;
        }
        add_project_files(&mut desired, &config, license, adoption_date)?;
        compare(desired, view)
    }

    pub fn plan_project_files(
        view: &dyn ProjectView,
        config: &EffectiveConfig,
        license: &VerifiedLicense,
        initial_adoption_date: UtcDate,
    ) -> Result<ChangePlan, MaterialsError> {
        validate_license(config, license)?;
        let adoption_date = config
            .project()
            .adoption_date()
            .unwrap_or(initial_adoption_date);
        let mut desired = ChangePlan::new();
        add_project_files(&mut desired, config, license, adoption_date)?;
        compare(desired, view)
    }

    pub fn plan_license_sync(
        view: &dyn ProjectView,
        config: &EffectiveConfig,
        license: &VerifiedLicense,
    ) -> Result<ChangePlan, MaterialsError> {
        validate_license(config, license)?;
        let layout = LayoutPolicy::from_config(config)?;
        let mut desired = ChangePlan::new();
        write(
            &mut desired,
            layout.official_license_path(&license.source_filename)?,
            license.body.as_bytes().to_vec(),
        )?;
        compare(desired, view)
    }
}

fn add_project_files(
    plan: &mut ChangePlan,
    config: &EffectiveConfig,
    license: &VerifiedLicense,
    adoption_date: UtcDate,
) -> Result<(), MaterialsError> {
    let layout = LayoutPolicy::from_config(config)?;
    write(
        plan,
        repo_path("LICENSE")?,
        render::root_license(config, &layout, license).into_bytes(),
    )?;
    write(
        plan,
        layout.official_license_path(&license.source_filename)?,
        license.body.as_bytes().to_vec(),
    )?;
    write(
        plan,
        layout.project_notice_path()?,
        render::project_notice(config, &layout, adoption_date).into_bytes(),
    )?;
    write(
        plan,
        layout.version_adoption_path()?,
        render::version_adoption(config, adoption_date).into_bytes(),
    )?;
    write(
        plan,
        layout.source_path()?,
        render::source(config).into_bytes(),
    )?;
    write(
        plan,
        layout.dependencies_path()?,
        render::empty_dependencies(config).into_bytes(),
    )?;
    if layout.requires_empty_restrictions_index() {
        write(
            plan,
            layout.restrictions_index_path()?,
            b"No Additional Restrictions are effective.\n".to_vec(),
        )?;
    }
    if layout.requires_special_authorizations_placeholder()
        || !config
            .license()
            .special_authorization_channel()
            .trim()
            .is_empty()
    {
        write(
            plan,
            layout.special_authorizations_path()?,
            render::special_authorizations(config).into_bytes(),
        )?;
    }
    Ok(())
}

fn render_config(
    identity: &ProjectIdentity,
    version: AhclVersion,
    current_date: UtcDate,
) -> Result<ConfigDocument, MaterialsError> {
    let mut document = ConfigDocument::parse(&ConfigSkeleton::render_populated(identity))
        .map_err(map_config_error)?;
    let (version, directory) = match version {
        AhclVersion::V1_0 => ("1.0", "AHCL"),
        AhclVersion::V1_1 => ("1.1", ".ahcl"),
    };
    document
        .upsert_scalar(
            None,
            "materials-directory",
            ScalarValue::String(directory.to_owned()),
        )
        .map_err(map_config_error)?;
    document
        .upsert_scalar(
            Some("license"),
            "version",
            ScalarValue::String(version.to_owned()),
        )
        .map_err(map_config_error)?;
    document
        .upsert_scalar(
            Some("project"),
            "adoption-date",
            ScalarValue::String(current_date.to_string()),
        )
        .map_err(map_config_error)?;
    EffectiveConfig::resolve(&document).map_err(map_config_error)?;
    Ok(document)
}

fn complete_identity(identity: &ProjectIdentity) -> bool {
    !identity.name().trim().is_empty()
        && !identity.canonical_repository().trim().is_empty()
        && !identity.right_holders().is_empty()
        && identity
            .right_holders()
            .iter()
            .all(|holder| !holder.trim().is_empty())
}

fn complete_config_identity(config: &EffectiveConfig) -> bool {
    !config.project().name().trim().is_empty()
        && !config.project().canonical_repository().trim().is_empty()
        && !config.project().right_holders().is_empty()
        && config
            .project()
            .right_holders()
            .iter()
            .all(|holder| !holder.trim().is_empty())
}

fn validate_license(
    config: &EffectiveConfig,
    license: &VerifiedLicense,
) -> Result<(), MaterialsError> {
    if config.license().version() != license.version {
        return Err(MaterialsError::new(
            MaterialsErrorCode::LicenseVersionMismatch,
        ));
    }
    Ok(())
}

fn compare(plan: ChangePlan, view: &dyn ProjectView) -> Result<ChangePlan, MaterialsError> {
    plan.compare(view).map_err(map_plan_error)
}

fn write(plan: &mut ChangePlan, path: RepoPath, bytes: Vec<u8>) -> Result<(), MaterialsError> {
    plan.write(path, bytes).map_err(map_plan_error)
}

fn repo_path(value: &str) -> Result<RepoPath, MaterialsError> {
    RepoPath::parse(value).map_err(|_| MaterialsError::new(MaterialsErrorCode::InvalidLayout))
}

fn map_config_error(_: ConfigError) -> MaterialsError {
    MaterialsError::new(MaterialsErrorCode::InvalidConfig)
}

fn map_plan_error(error: PlanError) -> MaterialsError {
    let code = match error {
        PlanError::View { .. } => MaterialsErrorCode::View,
        PlanError::ConflictingChange { .. }
        | PlanError::MissingBytes { .. }
        | PlanError::UnsupportedEntry { .. } => MaterialsErrorCode::Plan,
    };
    MaterialsError::new(code)
}
