// crates/ahcl-kit-materials/src/project.rs - Project material generation and material errors.
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

use crate::LayoutPolicy;
use crate::render;
use ahcl_kit_config::{
    AhclVersion, ConfigDocument, ConfigError, ConfigSkeleton, EffectiveConfig, ProjectIdentity,
    ScalarValue,
};
use ahcl_kit_core::{ChangePlan, PlanError, ProjectEntry, ProjectView, RepoPath, UtcDate};
use ahcl_kit_license::VerifiedLicense;
use std::collections::BTreeMap;
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
    AmbiguousManagedContent,
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
            Self::AmbiguousManagedContent => "materials.managed_ambiguous",
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
        MaterialsErrorCode::AmbiguousManagedContent => {
            "managed AHCL content is ambiguous or malformed"
        }
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
        add_project_files(view, &mut desired, &config, license, adoption_date)?;
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
        let mut licenses = Vec::new();
        let mut notices = Vec::new();
        let mut sources = Vec::new();
        let mut adoptions = Vec::new();
        for scope in scopes {
            validate_license(scope, license)?;
            let layout = LayoutPolicy::from_config(scope)?;
            if scope.license().version() != AhclVersion::V1_2
                || layout.materials_directory() != first_layout.materials_directory()
            {
                return Err(MaterialsError::new(MaterialsErrorCode::InvalidLayout));
            }
            if scope.license().enabled() {
                licenses.push(render::root_license(scope, &layout, license));
                notices.push(render::managed_block(
                    scope,
                    &render::project_notice(scope, &layout, adoption_date),
                ));
                sources.push(render::managed_block(scope, &render::source(scope)));
                adoptions.push(render::version_adoption(scope, adoption_date));
            }
        }
        let mut desired = ChangePlan::new();
        write_managed(
            view,
            &mut desired,
            repo_path("LICENSE")?,
            &licenses,
            ManagedFileKind::License,
        )?;
        write_managed(
            view,
            &mut desired,
            first_layout.project_notice_path()?,
            &notices,
            ManagedFileKind::Document,
        )?;
        write_managed(
            view,
            &mut desired,
            first_layout.source_path()?,
            &sources,
            ManagedFileKind::Document,
        )?;
        write_managed(
            view,
            &mut desired,
            first_layout.version_adoption_path()?,
            &adoptions,
            ManagedFileKind::Adoption,
        )?;
        if scopes.iter().any(|scope| scope.license().enabled()) {
            write_managed(
                view,
                &mut desired,
                first_layout.official_license_path(&license.source_filename)?,
                std::slice::from_ref(&license.body),
                ManagedFileKind::Official,
            )?;
        }
        compare(desired, view)
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

fn add_project_files(
    view: &dyn ProjectView,
    plan: &mut ChangePlan,
    config: &EffectiveConfig,
    license: &VerifiedLicense,
    adoption_date: UtcDate,
) -> Result<(), MaterialsError> {
    let layout = LayoutPolicy::from_config(config)?;
    if config.license().enabled() {
        write_managed(
            view,
            plan,
            repo_path("LICENSE")?,
            &[render::root_license(config, &layout, license)],
            ManagedFileKind::License,
        )?;
        write_managed(
            view,
            plan,
            layout.official_license_path(&license.source_filename)?,
            std::slice::from_ref(&license.body),
            ManagedFileKind::Official,
        )?;
        write_managed(
            view,
            plan,
            layout.project_notice_path()?,
            &[document_body(
                config,
                render::project_notice(config, &layout, adoption_date),
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
    }
    write_managed(
        view,
        plan,
        layout.dependencies_path()?,
        &[render::empty_dependencies(config)],
        ManagedFileKind::Dependency,
    )?;
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
    if layout.requires_special_authorizations_placeholder()
        || !config
            .license()
            .special_authorization_channel()
            .trim()
            .is_empty()
    {
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

fn document_body(config: &EffectiveConfig, body: String) -> String {
    if config.license().version() == AhclVersion::V1_2 {
        render::managed_block(config, &body)
    } else {
        body
    }
}

#[derive(Clone, Copy)]
enum ManagedFileKind {
    License,
    Document,
    Adoption,
    Official,
    Dependency,
}

#[derive(Clone, Debug)]
struct ManagedRange {
    start: usize,
    end: usize,
    key: String,
}

fn write_managed(
    view: &dyn ProjectView,
    plan: &mut ChangePlan,
    path: RepoPath,
    generated: &[String],
    kind: ManagedFileKind,
) -> Result<(), MaterialsError> {
    if generated.is_empty() {
        return Ok(());
    }
    let desired = generated.join("\n");
    let existing = view
        .entry(&path)
        .map_err(|_| MaterialsError::new(MaterialsErrorCode::View))?;
    let bytes = match existing {
        ProjectEntry::Absent => desired.into_bytes(),
        ProjectEntry::File(existing) => {
            let existing = String::from_utf8(existing)
                .map_err(|_| MaterialsError::new(MaterialsErrorCode::AmbiguousManagedContent))?;
            let Some(merged) = merge_managed(&existing, &desired, kind)? else {
                return Ok(());
            };
            merged.into_bytes()
        }
        ProjectEntry::Other => {
            return Err(MaterialsError::new(MaterialsErrorCode::Plan));
        }
    };
    write(plan, path, bytes)
}

fn merge_managed(
    existing: &str,
    desired: &str,
    kind: ManagedFileKind,
) -> Result<Option<String>, MaterialsError> {
    if matches!(
        kind,
        ManagedFileKind::Official | ManagedFileKind::Dependency
    ) {
        return Ok(None);
    }
    let desired_ranges = managed_ranges(desired, kind)?;
    if desired_ranges.is_empty() {
        return Ok(None);
    }
    let ranges = managed_ranges(existing, kind)?;
    if ranges.is_empty() {
        let separator = if existing.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        return Ok(Some(format!("{existing}{separator}{desired}")));
    }
    let mut desired_by_key = BTreeMap::new();
    for range in &desired_ranges {
        let value = desired
            .get(range.start..range.end)
            .ok_or_else(|| MaterialsError::new(MaterialsErrorCode::AmbiguousManagedContent))?;
        if desired_by_key.insert(range.key.clone(), value).is_some() {
            return Err(MaterialsError::new(
                MaterialsErrorCode::AmbiguousManagedContent,
            ));
        }
    }
    let mut existing_keys = BTreeMap::new();
    for range in &ranges {
        if existing_keys.insert(range.key.clone(), range).is_some() {
            return Err(MaterialsError::new(
                MaterialsErrorCode::AmbiguousManagedContent,
            ));
        }
    }
    let mut output = String::with_capacity(existing.len() + desired.len());
    let mut cursor = 0;
    for range in &ranges {
        output.push_str(&existing[cursor..range.start]);
        let old = &existing[range.start..range.end];
        if let Some(replacement) = desired_by_key.get(&range.key) {
            if matches!(kind, ManagedFileKind::Adoption) && !old.contains("Effective Date:") {
                let end_marker =
                    old.rfind("<!-- END AHCL KIT MANAGED SCOPE:")
                        .ok_or_else(|| {
                            MaterialsError::new(MaterialsErrorCode::AmbiguousManagedContent)
                        })?;
                let desired_range = desired_ranges
                    .iter()
                    .find(|candidate| candidate.key == range.key)
                    .ok_or_else(|| MaterialsError::new(MaterialsErrorCode::Plan))?;
                let desired_block = desired
                    .get(desired_range.start..desired_range.end)
                    .ok_or_else(|| MaterialsError::new(MaterialsErrorCode::Plan))?;
                let event_start = desired_block
                    .find('\n')
                    .map(|index| index + 1)
                    .unwrap_or(desired_block.len());
                let event_end = desired_block
                    .rfind("<!-- END AHCL KIT MANAGED SCOPE:")
                    .unwrap_or(desired_block.len());
                let event = desired_block[event_start..event_end].trim();
                output.push_str(&old[..end_marker]);
                if !old.contains(event) {
                    output.push('\n');
                    output.push_str(event);
                    output.push('\n');
                }
                output.push_str(&old[end_marker..]);
            } else {
                output.push_str(replacement);
            }
        } else {
            output.push_str(old);
        }
        cursor = range.end;
    }
    output.push_str(&existing[cursor..]);
    for range in &desired_ranges {
        if !existing_keys.contains_key(&range.key) {
            if !output.ends_with('\n') {
                output.push('\n');
            }
            output.push('\n');
            output.push_str(&desired[range.start..range.end]);
        }
    }
    if output == existing {
        Ok(None)
    } else {
        Ok(Some(output))
    }
}

fn managed_ranges(
    content: &str,
    kind: ManagedFileKind,
) -> Result<Vec<ManagedRange>, MaterialsError> {
    let legal = matches!(kind, ManagedFileKind::License);
    let mut ranges = Vec::new();
    let mut active: Option<(usize, String)> = None;
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        let text = line
            .strip_suffix('\n')
            .unwrap_or(line)
            .trim_end_matches('\r');
        let begin = if legal {
            text == "----- BEGIN AHCL NOTICE -----"
        } else {
            text.starts_with("<!-- BEGIN AHCL KIT MANAGED SCOPE: ") && text.ends_with(" -->")
        };
        let end = if legal {
            text == "----- END AHCL NOTICE -----"
        } else {
            text.starts_with("<!-- END AHCL KIT MANAGED SCOPE: ") && text.ends_with(" -->")
        };
        if begin {
            if active.is_some() {
                return Err(MaterialsError::new(
                    MaterialsErrorCode::AmbiguousManagedContent,
                ));
            }
            let key = if legal {
                String::new()
            } else {
                text.trim_start_matches("<!-- BEGIN AHCL KIT MANAGED SCOPE: ")
                    .trim_end_matches(" -->")
                    .to_owned()
            };
            active = Some((offset, key));
        } else if end {
            let Some((start, mut key)) = active.take() else {
                return Err(MaterialsError::new(
                    MaterialsErrorCode::AmbiguousManagedContent,
                ));
            };
            if legal {
                let body = content.get(start..offset + line.len()).ok_or_else(|| {
                    MaterialsError::new(MaterialsErrorCode::AmbiguousManagedContent)
                })?;
                let marker = "<!-- AHCL KIT MANAGED SCOPE:";
                if body.matches(marker).count() > 1 {
                    return Err(MaterialsError::new(
                        MaterialsErrorCode::AmbiguousManagedContent,
                    ));
                }
                let Some(marker_start) = body.find(marker) else {
                    key = String::new();
                    ranges.push(ManagedRange {
                        start,
                        end: offset + line.len(),
                        key,
                    });
                    offset += line.len();
                    continue;
                };
                let marker_line = body[marker_start..].lines().next().unwrap_or_default();
                key = marker_line
                    .trim_start_matches(marker)
                    .trim()
                    .trim_end_matches("-->")
                    .trim()
                    .to_owned();
            } else {
                let end_key = text
                    .trim_start_matches("<!-- END AHCL KIT MANAGED SCOPE: ")
                    .trim_end_matches(" -->");
                if key != end_key {
                    return Err(MaterialsError::new(
                        MaterialsErrorCode::AmbiguousManagedContent,
                    ));
                }
            }
            ranges.push(ManagedRange {
                start,
                end: offset + line.len(),
                key,
            });
        }
        offset += line.len();
    }
    if active.is_some() {
        return Err(MaterialsError::new(
            MaterialsErrorCode::AmbiguousManagedContent,
        ));
    }
    Ok(ranges)
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
        AhclVersion::V1_2 => ("1.2", ".ahcl"),
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
