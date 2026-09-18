use crate::{MaterialsError, MaterialsErrorCode};
use ahcl_kit_config::{AhclVersion, EffectiveConfig};
use ahcl_kit_core::RepoPath;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayoutPolicy {
    version: AhclVersion,
    materials_directory: RepoPath,
}

impl LayoutPolicy {
    pub fn from_config(config: &EffectiveConfig) -> Result<Self, MaterialsError> {
        Self::new(
            config.license().version(),
            config.materials_directory().clone(),
        )
    }

    pub fn new(
        version: AhclVersion,
        materials_directory: RepoPath,
    ) -> Result<Self, MaterialsError> {
        let valid = match version {
            AhclVersion::V1_0 => materials_directory.as_str() == "AHCL",
            AhclVersion::V1_1 => matches!(
                materials_directory.as_str(),
                "AHCL" | "licenses/AHCL" | ".AHCL" | ".ahcl"
            ),
        };
        if !valid {
            return Err(MaterialsError::new(MaterialsErrorCode::InvalidLayout));
        }
        Ok(Self {
            version,
            materials_directory,
        })
    }

    pub fn version(&self) -> AhclVersion {
        self.version
    }

    pub fn materials_directory(&self) -> &RepoPath {
        &self.materials_directory
    }

    pub fn official_license_path(&self, filename: &str) -> Result<RepoPath, MaterialsError> {
        if filename.contains(['/', '\\']) || filename.is_empty() {
            return Err(MaterialsError::new(MaterialsErrorCode::InvalidLayout));
        }
        self.material_path(filename)
    }

    pub fn project_notice_path(&self) -> Result<RepoPath, MaterialsError> {
        self.material_path("AHCL-PROJECT-NOTICE.md")
    }

    pub fn version_adoption_path(&self) -> Result<RepoPath, MaterialsError> {
        self.material_path("AHCL-VERSION-ADOPTION.md")
    }

    pub fn source_path(&self) -> Result<RepoPath, MaterialsError> {
        self.material_path("AHCL-SOURCE.md")
    }

    pub fn dependencies_path(&self) -> Result<RepoPath, MaterialsError> {
        self.material_path("AHCL-DEPENDENCIES.md")
    }

    pub fn restrictions_index_path(&self) -> Result<RepoPath, MaterialsError> {
        self.material_path("AHCL-RESTRICTIONS/INDEX.md")
    }

    pub fn special_authorizations_path(&self) -> Result<RepoPath, MaterialsError> {
        self.material_path("AHCL-SPECIAL-AUTHORIZATIONS.md")
    }

    pub(crate) fn requires_empty_restrictions_index(&self) -> bool {
        self.version == AhclVersion::V1_0
    }

    pub(crate) fn requires_special_authorizations_placeholder(&self) -> bool {
        self.version == AhclVersion::V1_0
    }

    fn material_path(&self, relative: &str) -> Result<RepoPath, MaterialsError> {
        RepoPath::parse(format!("{}/{relative}", self.materials_directory.as_str()))
            .map_err(|_| MaterialsError::new(MaterialsErrorCode::InvalidLayout))
    }
}
