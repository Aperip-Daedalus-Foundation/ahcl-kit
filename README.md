<div align="center">

<p>
  <img src="assets/branding/ahcl-kit.svg" width="144" height="144" alt="AHCL Kit icon">
</p>

# AHCL Kit

<p>
  <img src="https://img.shields.io/github/v/release/Aperip-Daedalus-Foundation/ahcl-kit?display_name=tag&amp;style=flat-square&amp;label=release" alt="Latest release">
  <img src="https://img.shields.io/github/stars/Aperip-Daedalus-Foundation/ahcl-kit?style=flat-square&amp;label=stars" alt="Repository stars">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-334155?style=flat-square" alt="Supported platforms">
  <img src="https://img.shields.io/badge/license-AHCL%201.1-0F766E?style=flat-square" alt="AHCL 1.1 license">
</p>

English | [简体中文](docs/README.zh-CN.md) | [日本語](docs/README.ja.md)

AHCL Kit initializes, generates, and maintains AHCL materials for projects. It
uses a project-owned `.ahclkitconfigs` file, produces deterministic output, and
keeps license downloads and dependency evidence behind explicit commands.

The current adapter supports only Rust projects managed by Cargo. Support for
other package managers is planned.

## Install

Release assets provide native packages for the following systems:

| System | Architectures | Package |
| --- | --- | --- |
| Windows | x64, ARM64 | MSI |
| macOS 11 or later | Intel, Apple Silicon | Universal PKG |
| Debian/Ubuntu Linux | x86-64, ARM64 | DEB |
| RPM-based Linux | x86-64, ARM64 | RPM |

Download the package for the target system from GitHub Releases and install it
with the platform package manager. After installation, open a new terminal and
run:

```console
ahcl --version
```

## Quick Start

Create a complete configuration skeleton in the current project:

```console
ahcl config init
```

Edit `.ahclkitconfigs`, then initialize the AHCL Materials Directory and selected
license version:

```console
ahcl project init
```

Generate every configured material:

```console
ahcl project generate
```

Preview changes without writing:

```console
ahcl project generate --dry-run
```

Check whether generated files are current:

```console
ahcl project check
```

Commands use the current directory when no project path is supplied. AHCL Kit
discovers `.ahclkitconfigs` upward for normal commands and supports explicit
project paths and project-list files for batch operation.

## Commands

| Command | Purpose |
| --- | --- |
| `ahcl config init` | Create a complete configuration skeleton with no languages selected. |
| `ahcl config validate` | Validate project configuration. |
| `ahcl config show` | Show the resolved configuration and defaults. |
| `ahcl project init` | Initialize configuration, license, and the AHCL Materials Directory. |
| `ahcl project generate` | Generate all configured project materials. |
| `ahcl project check` | Report drift without modifying files. |
| `ahcl license sync` | Download and verify the selected official AHCL license. |
| `ahcl dependency generate` | Generate `[materials-directory]/AHCL-DEPENDENCIES.md`. |
| `ahcl third-party generate` | Generate per-package evidence under `[materials-directory]/THIRD-PARTY-LICENSES/`. |

Writing commands support `--dry-run`. All project commands support batch input;
run `ahcl <command> --help` for the accepted flags and output formats.

## Configuration

```text
schema = 1
materials-directory = ".ahcl"
languages:
  - "rust"

[project]
name = "Example"
canonical-repository = "https://example.com/example.git"
canonical-branch = "main"
right-holders:
  - "Example Foundation"
contact = ""
adoption-date = "2026-09-20"

[license]
version = "1.1"
special-authorization-channel = ""

[generation]
strict-license-files = true

[rust.cargo]
manifests:
  - "Cargo.toml"
packages = []
rules = []
```

Omitted `schema` uses the latest supported schema, omitted
`materials-directory` uses `.ahcl`, and omitted languages resolve to an empty
set. Empty languages allow the generic AHCL project material commands to run
without invoking an ecosystem adapter.

## Build From Source

The repository pins its Rust toolchain. Build the executable with:

```console
cargo build --locked --release --package ahcl-kit-cli --bin ahcl
```

## License

AHCL Kit uses AHCL 1.1. Before using, modifying, or distributing the project,
read [LICENSE](LICENSE) and the complete [AHCL Materials Directory](.ahcl/).

</div>
