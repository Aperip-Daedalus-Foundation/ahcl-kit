// assets/packaging/src/platform/windows_resources.rs - Windows executable resources.
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

use crate::command;
use crate::metadata::ProductMetadata;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

// ahcl-kit-cli used to own the ahcl binary and embed these resources with
// winresource. The binary now lives in the composition root, which has no
// build script. Packaging links the same resource file into the distributed exe.
pub(crate) fn compile_resources(
    root: &Path,
    product: &ProductMetadata,
    work: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    let script_path = work.join("ahcl.rc");
    let resource_path = work.join("ahcl.res");
    let icon = root.join("assets/branding/ahcl-kit.ico");
    fs::write(&script_path, resource_script(product, &icon)?)?;
    command::run(
        Command::new(resource_compiler()?)
            .arg("/fo")
            .arg(&resource_path)
            .arg(&script_path),
        "compile Windows executable resources",
    )?;
    Ok(resource_path)
}

pub(crate) fn icon_sample(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    let bytes = fs::read(path)?;
    let offset = icon_image_offset(&bytes)?;
    let end = offset.saturating_add(48).min(bytes.len());
    if end.saturating_sub(offset) < 16 {
        return Err(io::Error::other("Windows icon image is too small").into());
    }
    Ok(bytes[offset..end].to_vec())
}

fn resource_script(product: &ProductMetadata, icon: &Path) -> Result<String, Box<dyn Error>> {
    let quad = version_quad(&product.version)?;
    let mut script = String::new();
    push_version_header(&mut script, &quad);
    script.push_str(&version_strings(product));
    script.push_str("}\n}\n");
    script.push_str("BLOCK \"VarFileInfo\" {\n");
    script.push_str("VALUE \"Translation\", 0x0, 0x04b0\n");
    script.push_str("}\n}\n");
    let icon = utf8_path(icon)?;
    script.push_str(&format!("1 ICON \"{}\"\n", escape_rc(icon)));
    script.push_str(&manifest_resource());
    Ok(script)
}

fn push_version_header(script: &mut String, quad: &str) {
    script.push_str("#pragma code_page(65001)\n1 VERSIONINFO\n");
    script.push_str(&format!("FILEVERSION {quad}\nPRODUCTVERSION {quad}\n"));
    script.push_str("FILEOS 0x40004\nFILETYPE 0x1\nFILESUBTYPE 0x0\n");
    script.push_str("FILEFLAGSMASK 0x3f\nFILEFLAGS 0x0\n{\n");
    script.push_str("BLOCK \"StringFileInfo\"\n{\nBLOCK \"000004b0\"\n{\n");
}

fn version_strings(product: &ProductMetadata) -> String {
    let original = format!("{}.exe", product.binary_name);
    let fields = [
        ("Comments", product.homepage.as_str()),
        ("CompanyName", product.publisher.as_str()),
        ("FileDescription", product.product_name.as_str()),
        ("FileVersion", product.version.as_str()),
        ("InternalName", product.binary_name.as_str()),
        ("LegalCopyright", product.copyright.as_str()),
        ("OriginalFilename", original.as_str()),
        ("ProductName", product.product_name.as_str()),
        ("ProductVersion", product.version.as_str()),
    ];
    fields
        .into_iter()
        .map(|(name, value)| format!("VALUE \"{}\", \"{}\"\n", escape_rc(name), escape_rc(value)))
        .collect()
}

fn version_quad(version: &str) -> Result<String, Box<dyn Error>> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 {
        return Err(io::Error::other("product version must be major.minor.patch").into());
    }
    let major = parse_version_part(parts[0])?;
    let minor = parse_version_part(parts[1])?;
    let patch = parse_version_part(parts[2])?;
    Ok(format!("{major}, {minor}, {patch}, 0"))
}

fn parse_version_part(part: &str) -> Result<u16, Box<dyn Error>> {
    part.parse::<u16>()
        .map_err(|_| io::Error::other(format!("invalid product version part: {part}")).into())
}

fn manifest_resource() -> String {
    let mut block = String::from("1 24\n{\n");
    for line in MANIFEST.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        block.push_str(&format!("\" {} \"\n", escape_rc(trimmed)));
    }
    block.push_str("}\n");
    block
}

fn escape_rc(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        push_escaped(&mut escaped, character);
    }
    escaped
}

fn push_escaped(escaped: &mut String, character: char) {
    match character {
        '"' => escaped.push_str("\"\""),
        '\\' => escaped.push_str("\\\\"),
        '\n' => escaped.push_str("\\n"),
        '\t' => escaped.push_str("\\t"),
        '\r' => escaped.push_str("\\r"),
        _ => escaped.push(character),
    }
}

fn resource_compiler() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = std::env::var_os("RC_PATH") {
        return Ok(PathBuf::from(path));
    }
    newest_kit_compiler()
}

fn newest_kit_compiler() -> Result<PathBuf, Box<dyn Error>> {
    let mut compilers = kit_compilers(Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin"))?;
    compilers.sort();
    compilers
        .pop()
        .ok_or_else(|| io::Error::other("Windows resource compiler rc.exe was not found").into())
}

fn kit_compilers(root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut compilers = Vec::new();
    for entry in fs::read_dir(root)? {
        if let Some(compiler) = versioned_compiler(&entry?.path()) {
            compilers.push(compiler);
        }
    }
    Ok(compilers)
}

fn versioned_compiler(dir: &Path) -> Option<PathBuf> {
    let name = dir.file_name()?.to_str()?;
    if !name.starts_with("10.") {
        return None;
    }
    let compiler = dir.join(host_kit_arch()).join("rc.exe");
    compiler.is_file().then_some(compiler)
}

fn host_kit_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => "x86",
    }
}

fn utf8_path(path: &Path) -> Result<&str, Box<dyn Error>> {
    path.to_str()
        .ok_or_else(|| io::Error::other("Windows resource path must be UTF-8").into())
}

fn icon_image_offset(bytes: &[u8]) -> Result<usize, Box<dyn Error>> {
    if bytes.len() < 22 || bytes[0..4] != [0, 0, 1, 0] {
        return Err(io::Error::other("Windows icon resource is invalid").into());
    }
    let offset = u32::from_le_bytes(bytes[18..22].try_into().expect("icon offset is 4 bytes"));
    if offset as usize >= bytes.len() {
        return Err(io::Error::other("Windows icon image is out of range").into());
    }
    Ok(offset as usize)
}

const MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
    </windowsSettings>
  </application>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges><requestedExecutionLevel level="asInvoker" uiAccess="false"/></requestedPrivileges></security>
  </trustInfo>
</assembly>"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn product() -> ProductMetadata {
        ProductMetadata {
            version: "0.3.0".to_string(),
            description: "desc".to_string(),
            homepage: "https://ahcl.aperip.com".to_string(),
            product_name: "AHCL Kit".to_string(),
            binary_name: "ahcl".to_string(),
            publisher: "Aperip Daedalus Foundation".to_string(),
            copyright: "Copyright (C) 2026 Aperip Daedalus Foundation. All rights reserved."
                .to_string(),
            identifier: "com.aperip.ahcl-kit".to_string(),
            language: "neutral".to_string(),
        }
    }

    #[test]
    fn script_matches_previous_windows_fields() {
        let script = resource_script(&product(), Path::new(r"F:\icon.ico")).expect("script");
        assert!(script.contains("FILEVERSION 0, 3, 0, 0"));
        assert!(script.contains("PRODUCTVERSION 0, 3, 0, 0"));
        assert!(
            script.contains(
                "VALUE \"FileDescription\", \"AHCL Kit\"\nVALUE \"FileVersion\", \"0.3.0\""
            )
        );
        assert!(script.contains("VALUE \"LegalCopyright\", \"Copyright (C) 2026 Aperip Daedalus Foundation. All rights reserved.\""));
        assert!(script.contains("VALUE \"OriginalFilename\", \"ahcl.exe\""));
        assert!(script.contains("VALUE \"ProductName\", \"AHCL Kit\""));
        assert!(script.contains("1 ICON \"F:\\\\icon.ico\""));
        assert!(script.contains("longPathAware"));
        assert!(script.contains("activeCodePage"));
    }

    #[test]
    fn version_quad_requires_three_numbers() {
        assert!(version_quad("1.2").is_err());
        assert_eq!(version_quad("0.3.0").expect("quad"), "0, 3, 0, 0");
    }
}
