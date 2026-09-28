// crates/ahcl-kit-materials/src/project.rs - Project material generation and material errors.
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

use crate::LayoutPolicy;
use crate::render;
use ahcl_kit_config::{
    AhclVersion, ConfigDocument, ConfigError, ConfigSkeleton, EffectiveConfig, LanguageContributor,
    ProjectIdentity, ScalarValue,
};
use ahcl_kit_core::{ChangePlan, PlanError, ProjectEntry, ProjectView, RepoPath, UtcDate};
use ahcl_kit_license::VerifiedLicense;
use std::fmt;

#[path = "project_merge.rs"]
mod merge;

use merge::{ManagedFileKind, write_managed};

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
    AmbiguousManagedContent,
    Plan,
    View,
    Filesystem(&'static str),
}

impl MaterialsErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IdentityRequired
            | Self::ConfigExists
            | Self::InvalidConfig
            | Self::InvalidLayout
            | Self::LicenseVersionMismatch
            | Self::StateInvalid => config_error_code(self),
            Self::LinkOrReparsePoint
            | Self::ManagedTreeInvalid
            | Self::AmbiguousManagedContent
            | Self::Plan
            | Self::View
            | Self::Filesystem(_) => material_error_code(self),
        }
    }
}

fn config_error_code(code: MaterialsErrorCode) -> &'static str {
    match code {
        MaterialsErrorCode::IdentityRequired => "config.identity_required",
        MaterialsErrorCode::ConfigExists => "config.exists",
        MaterialsErrorCode::InvalidConfig => "config.invalid",
        MaterialsErrorCode::InvalidLayout => "materials.layout_invalid",
        MaterialsErrorCode::LicenseVersionMismatch => "license.version_mismatch",
        MaterialsErrorCode::StateInvalid => "materials.state_invalid",
        other => material_error_code(other),
    }
}

fn material_error_code(code: MaterialsErrorCode) -> &'static str {
    match code {
        MaterialsErrorCode::LinkOrReparsePoint => "materials.link_or_reparse",
        MaterialsErrorCode::ManagedTreeInvalid => "materials.managed_tree_invalid",
        MaterialsErrorCode::AmbiguousManagedContent => "materials.managed_ambiguous",
        MaterialsErrorCode::Plan => "materials.plan",
        MaterialsErrorCode::View => "materials.view",
        MaterialsErrorCode::Filesystem(code) => code,
        MaterialsErrorCode::IdentityRequired
        | MaterialsErrorCode::ConfigExists
        | MaterialsErrorCode::InvalidConfig
        | MaterialsErrorCode::InvalidLayout
        | MaterialsErrorCode::LicenseVersionMismatch
        | MaterialsErrorCode::StateInvalid => "config.identity_required",
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

    pub(crate) fn filesystem_at(code: &'static str, message: &'static str, path: RepoPath) -> Self {
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
        MaterialsErrorCode::IdentityRequired
        | MaterialsErrorCode::ConfigExists
        | MaterialsErrorCode::InvalidConfig
        | MaterialsErrorCode::InvalidLayout
        | MaterialsErrorCode::LicenseVersionMismatch
        | MaterialsErrorCode::StateInvalid => config_error_message(code),
        MaterialsErrorCode::LinkOrReparsePoint
        | MaterialsErrorCode::ManagedTreeInvalid
        | MaterialsErrorCode::AmbiguousManagedContent
        | MaterialsErrorCode::Plan
        | MaterialsErrorCode::View
        | MaterialsErrorCode::Filesystem(_) => material_error_message(code),
    }
}

fn config_error_message(code: MaterialsErrorCode) -> &'static str {
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
        other => material_error_message(other),
    }
}

fn material_error_message(code: MaterialsErrorCode) -> &'static str {
    match code {
        MaterialsErrorCode::LinkOrReparsePoint => {
            "managed third-party tree contains a link or reparse point"
        }
        MaterialsErrorCode::ManagedTreeInvalid => "managed third-party tree is invalid",
        MaterialsErrorCode::AmbiguousManagedContent => {
            "managed AHCL content is ambiguous or malformed"
        }
        MaterialsErrorCode::Plan => "AHCL material plan could not be constructed",
        MaterialsErrorCode::View => "project entries could not be observed",
        MaterialsErrorCode::Filesystem(_) => "filesystem operation failed",
        MaterialsErrorCode::IdentityRequired
        | MaterialsErrorCode::ConfigExists
        | MaterialsErrorCode::InvalidConfig
        | MaterialsErrorCode::InvalidLayout
        | MaterialsErrorCode::LicenseVersionMismatch
        | MaterialsErrorCode::StateInvalid => {
            "project identity is required before AHCL initialization"
        }
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
        contributors: &[&'static dyn LanguageContributor],
    ) -> Result<ChangePlan, MaterialsError> {
        let prepared =
            prepare_init_document(view, identity, license, current_date, force, contributors)?;
        commit_init_plan(view, prepared, license, current_date)
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
        add_project_files(view, &mut desired, config, license, adoption_date)?;
        compare(desired, view)
    }

    pub fn plan_centralized_files(
        view: &dyn ProjectView,
        scopes: &[EffectiveConfig],
        license: &VerifiedLicense,
        adoption_date: UtcDate,
    ) -> Result<ChangePlan, MaterialsError> {
        let first = scopes
            .first()
            .ok_or_else(|| MaterialsError::new(MaterialsErrorCode::Plan))?;
        let first_layout = LayoutPolicy::from_config(first)?;
        if first.license().version() != AhclVersion::V1_2 {
            return Err(MaterialsError::new(MaterialsErrorCode::InvalidLayout));
        }
        let bodies = collect_centralized_bodies(scopes, license, adoption_date, &first_layout)?;
        write_centralized_files(view, &first_layout, license, bodies)
    }

    pub fn plan_license_sync(
        view: &dyn ProjectView,
        config: &EffectiveConfig,
        license: &VerifiedLicense,
    ) -> Result<ChangePlan, MaterialsError> {
        validate_license(config, license)?;
        if !config.license().enabled() {
            return Ok(ChangePlan::new());
        }
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

struct PreparedInit {
    path: RepoPath,
    document: ConfigDocument,
    write_config: bool,
}

fn prepare_init_document(
    view: &dyn ProjectView,
    identity: Option<&ProjectIdentity>,
    license: &VerifiedLicense,
    current_date: UtcDate,
    force: bool,
    contributors: &[&'static dyn LanguageContributor],
) -> Result<PreparedInit, MaterialsError> {
    let path = repo_path(".ahclkitconfigs")?;
    let entry = view
        .entry(&path)
        .map_err(|_| MaterialsError::new(MaterialsErrorCode::View))?;
    let (document, write_config) =
        init_document_state(entry, identity, license, current_date, force, contributors)?;
    Ok(PreparedInit {
        path,
        document,
        write_config,
    })
}

fn init_document_state(
    entry: ProjectEntry,
    identity: Option<&ProjectIdentity>,
    license: &VerifiedLicense,
    current_date: UtcDate,
    force: bool,
    contributors: &[&'static dyn LanguageContributor],
) -> Result<(ConfigDocument, bool), MaterialsError> {
    match entry {
        ProjectEntry::Absent => fresh_init_document(identity, license, current_date),
        ProjectEntry::File(_) if identity.is_some() => {
            replaced_init_document(identity, license, current_date, force)
        }
        ProjectEntry::File(bytes) => existing_init_document(bytes, current_date, contributors),
        ProjectEntry::Other => Err(MaterialsError::new(MaterialsErrorCode::InvalidConfig)),
    }
}

fn fresh_init_document(
    identity: Option<&ProjectIdentity>,
    license: &VerifiedLicense,
    current_date: UtcDate,
) -> Result<(ConfigDocument, bool), MaterialsError> {
    let identity = required_identity(identity)?;
    Ok((
        render_config(identity, license.version, current_date)?,
        true,
    ))
}

fn required_identity(
    identity: Option<&ProjectIdentity>,
) -> Result<&ProjectIdentity, MaterialsError> {
    identity
        .filter(|identity| complete_identity(identity))
        .ok_or_else(|| MaterialsError::new(MaterialsErrorCode::IdentityRequired))
}

fn replaced_init_document(
    identity: Option<&ProjectIdentity>,
    license: &VerifiedLicense,
    current_date: UtcDate,
    force: bool,
) -> Result<(ConfigDocument, bool), MaterialsError> {
    if !force {
        return Err(MaterialsError::new(MaterialsErrorCode::ConfigExists));
    }
    fresh_init_document(identity, license, current_date)
}

fn existing_init_document(
    bytes: Vec<u8>,
    current_date: UtcDate,
    contributors: &[&'static dyn LanguageContributor],
) -> Result<(ConfigDocument, bool), MaterialsError> {
    let mut document =
        ConfigDocument::parse_bytes_with(&bytes, contributors).map_err(map_config_error)?;
    let config = EffectiveConfig::resolve(&document).map_err(map_config_error)?;
    if !complete_config_identity(&config) {
        return Err(MaterialsError::new(MaterialsErrorCode::IdentityRequired));
    }
    let write_config = ensure_adoption_date(&mut document, &config, current_date)?;
    Ok((document, write_config))
}

fn ensure_adoption_date(
    document: &mut ConfigDocument,
    config: &EffectiveConfig,
    current_date: UtcDate,
) -> Result<bool, MaterialsError> {
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
    Ok(write_config)
}

fn commit_init_plan(
    view: &dyn ProjectView,
    prepared: PreparedInit,
    license: &VerifiedLicense,
    current_date: UtcDate,
) -> Result<ChangePlan, MaterialsError> {
    let config = EffectiveConfig::resolve(&prepared.document).map_err(map_config_error)?;
    validate_license(&config, license)?;
    let adoption_date = config.project().adoption_date().unwrap_or(current_date);
    let mut desired = ChangePlan::new();
    if prepared.write_config {
        write(
            &mut desired,
            prepared.path,
            prepared.document.render().into_bytes(),
        )?;
    }
    add_project_files(view, &mut desired, &config, license, adoption_date)?;
    compare(desired, view)
}

struct CentralizedBodies {
    licenses: Vec<String>,
    notices: Vec<String>,
    sources: Vec<String>,
    adoptions: Vec<String>,
    include_official: bool,
}

fn collect_centralized_bodies(
    scopes: &[EffectiveConfig],
    license: &VerifiedLicense,
    adoption_date: UtcDate,
    first_layout: &LayoutPolicy,
) -> Result<CentralizedBodies, MaterialsError> {
    let mut bodies = CentralizedBodies {
        licenses: Vec::new(),
        notices: Vec::new(),
        sources: Vec::new(),
        adoptions: Vec::new(),
        include_official: false,
    };
    for scope in scopes {
        push_centralized_scope(&mut bodies, scope, license, adoption_date, first_layout)?;
    }
    Ok(bodies)
}

fn push_centralized_scope(
    bodies: &mut CentralizedBodies,
    scope: &EffectiveConfig,
    license: &VerifiedLicense,
    adoption_date: UtcDate,
    first_layout: &LayoutPolicy,
) -> Result<(), MaterialsError> {
    validate_license(scope, license)?;
    let layout = LayoutPolicy::from_config(scope)?;
    if !same_centralized_layout(scope, &layout, first_layout) {
        return Err(MaterialsError::new(MaterialsErrorCode::InvalidLayout));
    }
    if scope.license().enabled() {
        push_enabled_scope(bodies, scope, &layout, license, adoption_date);
    }
    Ok(())
}

fn same_centralized_layout(
    scope: &EffectiveConfig,
    layout: &LayoutPolicy,
    first_layout: &LayoutPolicy,
) -> bool {
    scope.license().version() == AhclVersion::V1_2
        && layout.materials_directory() == first_layout.materials_directory()
}

fn push_enabled_scope(
    bodies: &mut CentralizedBodies,
    scope: &EffectiveConfig,
    layout: &LayoutPolicy,
    license: &VerifiedLicense,
    adoption_date: UtcDate,
) {
    bodies.include_official = true;
    bodies
        .licenses
        .push(render::root_license(scope, layout, license));
    bodies.notices.push(render::managed_block(
        scope,
        &render::project_notice(scope, layout, adoption_date),
    ));
    bodies
        .sources
        .push(render::managed_block(scope, &render::source(scope)));
    bodies
        .adoptions
        .push(render::version_adoption(scope, adoption_date));
}

fn write_centralized_files(
    view: &dyn ProjectView,
    layout: &LayoutPolicy,
    license: &VerifiedLicense,
    bodies: CentralizedBodies,
) -> Result<ChangePlan, MaterialsError> {
    let mut desired = ChangePlan::new();
    write_centralized_license_files(view, &mut desired, layout, &bodies)?;
    write_centralized_source_files(view, &mut desired, layout, &bodies)?;
    write_official_license(view, &mut desired, layout, license, bodies.include_official)?;
    compare(desired, view)
}

fn write_centralized_license_files(
    view: &dyn ProjectView,
    desired: &mut ChangePlan,
    layout: &LayoutPolicy,
    bodies: &CentralizedBodies,
) -> Result<(), MaterialsError> {
    write_managed(
        view,
        desired,
        repo_path("LICENSE")?,
        &bodies.licenses,
        ManagedFileKind::License,
    )?;
    write_managed(
        view,
        desired,
        layout.project_notice_path()?,
        &bodies.notices,
        ManagedFileKind::Document,
    )?;
    Ok(())
}

fn write_centralized_source_files(
    view: &dyn ProjectView,
    desired: &mut ChangePlan,
    layout: &LayoutPolicy,
    bodies: &CentralizedBodies,
) -> Result<(), MaterialsError> {
    write_managed(
        view,
        desired,
        layout.source_path()?,
        &bodies.sources,
        ManagedFileKind::Document,
    )?;
    write_managed(
        view,
        desired,
        layout.version_adoption_path()?,
        &bodies.adoptions,
        ManagedFileKind::Adoption,
    )?;
    Ok(())
}

fn write_official_license(
    view: &dyn ProjectView,
    desired: &mut ChangePlan,
    layout: &LayoutPolicy,
    license: &VerifiedLicense,
    include_official: bool,
) -> Result<(), MaterialsError> {
    if include_official {
        write_managed(
            view,
            desired,
            layout.official_license_path(&license.source_filename)?,
            std::slice::from_ref(&license.body),
            ManagedFileKind::Official,
        )?;
    }
    Ok(())
}

fn add_project_files(
    view: &dyn ProjectView,
    plan: &mut ChangePlan,
    config: &EffectiveConfig,
    license: &VerifiedLicense,
    adoption_date: UtcDate,
) -> Result<(), MaterialsError> {
    let layout = LayoutPolicy::from_config(config)?;
    if config.license().enabled() {
        add_licensed_project_files(view, plan, config, license, adoption_date, &layout)?;
    }
    add_dependency_file(view, plan, config, &layout)?;
    add_optional_project_files(view, plan, config, &layout)
}

fn add_licensed_project_files(
    view: &dyn ProjectView,
    plan: &mut ChangePlan,
    config: &EffectiveConfig,
    license: &VerifiedLicense,
    adoption_date: UtcDate,
    layout: &LayoutPolicy,
) -> Result<(), MaterialsError> {
    add_license_root_files(view, plan, config, license, layout)?;
    add_project_document_files(view, plan, config, adoption_date, layout)
}

fn add_license_root_files(
    view: &dyn ProjectView,
    plan: &mut ChangePlan,
    config: &EffectiveConfig,
    license: &VerifiedLicense,
    layout: &LayoutPolicy,
) -> Result<(), MaterialsError> {
    write_managed(
        view,
        plan,
        repo_path("LICENSE")?,
        &[render::root_license(config, layout, license)],
        ManagedFileKind::License,
    )?;
    write_managed(
        view,
        plan,
        layout.official_license_path(&license.source_filename)?,
        std::slice::from_ref(&license.body),
        ManagedFileKind::Official,
    )?;
    Ok(())
}

fn add_project_document_files(
    view: &dyn ProjectView,
    plan: &mut ChangePlan,
    config: &EffectiveConfig,
    adoption_date: UtcDate,
    layout: &LayoutPolicy,
) -> Result<(), MaterialsError> {
    write_managed(
        view,
        plan,
        layout.project_notice_path()?,
        &[document_body(
            config,
            render::project_notice(config, layout, adoption_date),
        )],
        ManagedFileKind::Document,
    )?;
    write_managed(
        view,
        plan,
        layout.version_adoption_path()?,
        &[render::version_adoption(config, adoption_date)],
        ManagedFileKind::Adoption,
    )?;
    write_managed(
        view,
        plan,
        layout.source_path()?,
        &[document_body(config, render::source(config))],
        ManagedFileKind::Document,
    )?;
    Ok(())
}

fn add_dependency_file(
    view: &dyn ProjectView,
    plan: &mut ChangePlan,
    config: &EffectiveConfig,
    layout: &LayoutPolicy,
) -> Result<(), MaterialsError> {
    write_managed(
        view,
        plan,
        layout.dependencies_path()?,
        &[render::empty_dependencies(config)],
        ManagedFileKind::Dependency,
    )
}

fn add_optional_project_files(
    view: &dyn ProjectView,
    plan: &mut ChangePlan,
    config: &EffectiveConfig,
    layout: &LayoutPolicy,
) -> Result<(), MaterialsError> {
    if layout.requires_empty_restrictions_index() {
        write_managed(
            view,
            plan,
            layout.restrictions_index_path()?,
            &[document_body(
                config,
                "No Additional Restrictions are effective.\n".to_owned(),
            )],
            ManagedFileKind::Document,
        )?;
    }
    if requires_special_authorizations(layout, config) {
        write_managed(
            view,
            plan,
            layout.special_authorizations_path()?,
            &[document_body(
                config,
                render::special_authorizations(config),
            )],
            ManagedFileKind::Document,
        )?;
    }
    Ok(())
}

fn requires_special_authorizations(layout: &LayoutPolicy, config: &EffectiveConfig) -> bool {
    layout.requires_special_authorizations_placeholder()
        || !config
            .license()
            .special_authorization_channel()
            .trim()
            .is_empty()
}

fn document_body(config: &EffectiveConfig, body: String) -> String {
    if config.license().version() == AhclVersion::V1_2 {
        render::managed_block(config, &body)
    } else {
        body
    }
}

fn render_config(
    identity: &ProjectIdentity,
    version: AhclVersion,
    current_date: UtcDate,
) -> Result<ConfigDocument, MaterialsError> {
    let mut document = ConfigDocument::parse(&ConfigSkeleton::render_populated(identity))
        .map_err(map_config_error)?;
    apply_init_scalars(&mut document, version, current_date)?;
    EffectiveConfig::resolve(&document).map_err(map_config_error)?;
    Ok(document)
}

fn apply_init_scalars(
    document: &mut ConfigDocument,
    version: AhclVersion,
    current_date: UtcDate,
) -> Result<(), MaterialsError> {
    let (version, directory) = version_directory(version);
    upsert_init_scalar(document, None, "materials-directory", directory)?;
    upsert_init_scalar(document, Some("license"), "version", version)?;
    upsert_init_scalar(
        document,
        Some("project"),
        "adoption-date",
        &current_date.to_string(),
    )?;
    Ok(())
}

fn version_directory(version: AhclVersion) -> (&'static str, &'static str) {
    match version {
        AhclVersion::V1_0 => ("1.0", "AHCL"),
        AhclVersion::V1_1 => ("1.1", ".ahcl"),
        AhclVersion::V1_2 => ("1.2", ".ahcl"),
    }
}

fn upsert_init_scalar(
    document: &mut ConfigDocument,
    section: Option<&str>,
    key: &str,
    value: &str,
) -> Result<(), MaterialsError> {
    document
        .upsert_scalar(section, key, ScalarValue::String(value.to_owned()))
        .map_err(map_config_error)
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
