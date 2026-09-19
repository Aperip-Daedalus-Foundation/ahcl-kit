use crate::platform_fs::{ManagedDirectory, PlatformRoot};
use crate::{ManagedThirdPartyInventory, MaterialsError, SafeRelPath};
use ahcl_kit_core::{ProjectEntry, ProjectView, ProjectViewError, RepoPath};
use std::fmt;
use std::marker::PhantomData;
use std::path::Path;

pub struct ProjectFilesystem {
    pub(crate) root: PlatformRoot,
}

impl ProjectFilesystem {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MaterialsError> {
        PlatformRoot::open(path.as_ref()).map(|root| Self { root })
    }

    pub fn managed_third_party_dir(
        &self,
        materials_directory: &RepoPath,
    ) -> Result<ManagedThirdPartyDir<'_>, MaterialsError> {
        let namespace = SafeRelPath::managed_namespace(materials_directory)?;
        let directory = self.root.open_managed(&namespace)?;
        Ok(ManagedThirdPartyDir {
            directory,
            _owner: PhantomData,
        })
    }
}

impl fmt::Debug for ProjectFilesystem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProjectFilesystem { capability: .. }")
    }
}

impl ProjectView for ProjectFilesystem {
    fn entry(&self, path: &RepoPath) -> Result<ProjectEntry, ProjectViewError> {
        let safe_path = SafeRelPath::from_repo_path(path).map_err(to_view_error)?;
        self.root.read_entry(&safe_path).map_err(to_view_error)
    }
}

pub struct ManagedThirdPartyDir<'a> {
    directory: ManagedDirectory,
    _owner: PhantomData<&'a ProjectFilesystem>,
}

impl ManagedThirdPartyDir<'_> {
    pub fn inventory(&self) -> Result<ManagedThirdPartyInventory, MaterialsError> {
        self.directory.inventory()
    }

    pub(crate) fn ensure_present(&self) -> Result<(), MaterialsError> {
        self.directory.ensure_present()
    }

    pub(crate) fn remove_evidence(&self, path: &SafeRelPath) -> Result<(), MaterialsError> {
        self.directory.remove_file(path)
    }

    pub(crate) fn remove_empty_package(&self, path: &SafeRelPath) -> Result<(), MaterialsError> {
        self.directory.remove_empty_directory(path)
    }
}

impl fmt::Debug for ManagedThirdPartyDir<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ManagedThirdPartyDir { capability: .. }")
    }
}

fn to_view_error(error: MaterialsError) -> ProjectViewError {
    let message = match error.path() {
        Some(path) => format!("{}: {}: {path}", error.code(), error.message()),
        None => format!("{}: {}", error.code(), error.message()),
    };
    ProjectViewError::new(message)
}
