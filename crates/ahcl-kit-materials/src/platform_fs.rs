#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(crate) use unix::{ManagedDirectory, PlatformRoot};
#[cfg(windows)]
pub(crate) use windows::{ManagedDirectory, PlatformRoot};
