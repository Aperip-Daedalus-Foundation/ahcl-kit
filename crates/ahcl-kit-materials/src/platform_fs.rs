use crate::MaterialsError;

pub(crate) const MAX_MANAGED_ROOT_ENTRIES: usize = 2_048;
pub(crate) const MAX_MANAGED_PACKAGES: usize = 1_024;
pub(crate) const MAX_MANAGED_EVIDENCE_PER_PACKAGE: usize = 64;
pub(crate) const MAX_MANAGED_TOTAL_ENTRIES: usize = 8_192;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(crate) use unix::{ManagedDirectory, PlatformRoot};
#[cfg(windows)]
pub(crate) use windows::{ManagedDirectory, PlatformRoot};

fn inventory_limit_error() -> MaterialsError {
    MaterialsError::filesystem(
        "materials.managed.inventory_limit",
        "managed third-party inventory exceeds supported entry limits",
    )
}
