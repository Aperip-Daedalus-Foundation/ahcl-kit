// assets/packaging/src/fsutil.rs - Distribution staging filesystem helpers.
// Copyright (C) 2026 Aperip Daedalus Foundation. All rights reserved.
// SPDX-License-Identifier: LicenseRef-AHCL-1.1

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(prefix: &str) -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let base = ["RUNNER_TEMP", "TMP", "TEMP"]
            .into_iter()
            .find_map(std::env::var_os)
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        fs::create_dir_all(&base)?;
        let path = base.join(format!("{prefix}-{}-{nonce}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub fn output_directory(root: &Path, output: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let path = if output.is_absolute() {
        output.to_owned()
    } else {
        root.join(output)
    };
    fs::create_dir_all(&path)?;
    Ok(path)
}

pub fn copy_file(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination)?;
    Ok(())
}

pub fn copy_tree(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    // Classify from symlink metadata so a link is copied as a link and never followed.
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return copy_symlink(source, destination);
    }
    if metadata.is_file() {
        return copy_file(source, destination);
    }
    copy_directory(&metadata, source, destination)
}

fn copy_symlink(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    let target = fs::read_link(source)?;
    create_symlink(&target, destination)
}

fn copy_directory(
    metadata: &fs::Metadata,
    source: &Path,
    destination: &Path,
) -> Result<(), Box<dyn Error>> {
    if !metadata.is_dir() {
        return Err(io::Error::other(format!(
            "unsupported staged file type: {}",
            source.display()
        ))
        .into());
    }
    fs::create_dir_all(destination)?;
    copy_sorted_children(source, destination)
}

fn copy_sorted_children(source: &Path, destination: &Path) -> Result<(), Box<dyn Error>> {
    for entry in sorted_dir_entries(source)? {
        copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
    }
    Ok(())
}

fn sorted_dir_entries(root: &Path) -> Result<Vec<fs::DirEntry>, Box<dyn Error>> {
    let mut entries = fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    Ok(entries)
}

pub fn render_template(
    source: &Path,
    destination: &Path,
    values: &BTreeMap<&str, String>,
) -> Result<(), Box<dyn Error>> {
    let mut document = fs::read_to_string(source)?;
    for (marker, value) in values {
        document = document.replace(marker, &xml_escape(value));
    }
    if document.contains('@') {
        return Err(io::Error::other(format!(
            "unresolved template marker in {}",
            source.display()
        ))
        .into());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(destination, document.replace("\r\n", "\n"))?;
    Ok(())
}

pub fn write_text(path: &Path, body: &str) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, body.replace("\r\n", "\n"))?;
    Ok(())
}

pub fn set_executable(path: &Path) -> Result<(), Box<dyn Error>> {
    set_executable_platform(path)
}

pub fn create_symlink(target: &Path, link: &Path) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = link.parent() {
        fs::create_dir_all(parent)?;
    }
    create_symlink_platform(target, link)
}

pub fn find_file(root: &Path, extension: &str) -> Result<PathBuf, Box<dyn Error>> {
    for entry in sorted_dir_entries(root)? {
        // A failed directory search is skipped so a later sibling can still match.
        if let Some(found) = find_in_entry(&entry.path(), extension) {
            return Ok(found);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("no .{extension} file under {}", root.display()),
    )
    .into())
}

fn find_in_entry(path: &Path, extension: &str) -> Option<PathBuf> {
    if path.is_dir() {
        return find_file(path, extension).ok();
    }
    if path.extension().and_then(|value| value.to_str()) == Some(extension) {
        return Some(path.to_path_buf());
    }
    None
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(unix)]
fn set_executable_platform(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_executable_platform(_path: &Path) -> Result<(), Box<dyn Error>> {
    Err(io::Error::other("Unix executable permissions are unavailable").into())
}

#[cfg(unix)]
fn create_symlink_platform(target: &Path, link: &Path) -> Result<(), Box<dyn Error>> {
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_symlink_platform(_target: &Path, _link: &Path) -> Result<(), Box<dyn Error>> {
    Err(io::Error::other("Unix symbolic links are unavailable").into())
}
