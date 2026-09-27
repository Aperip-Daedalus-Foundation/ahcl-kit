<div align="center">

<p>
  <img src="../assets/branding/ahcl-kit.svg" width="144" height="144" alt="AHCL Kit 图标">
</p>

# AHCL Kit

<p>
  <img src="https://img.shields.io/github/v/release/Aperip-Daedalus-Foundation/ahcl-kit?display_name=tag&amp;style=flat-square&amp;label=release" alt="最新版本">
  <img src="https://img.shields.io/github/stars/Aperip-Daedalus-Foundation/ahcl-kit?style=flat-square&amp;label=stars" alt="仓库 Star 数">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-334155?style=flat-square" alt="支持的平台">
  <img src="https://img.shields.io/badge/license-AHCL%201.1-0F766E?style=flat-square" alt="AHCL 1.1 许可证">
</p>

[English](../README.md) | 简体中文 | [日本語](README.ja.md)

AHCL Kit 用于为项目初始化、生成和维护 AHCL 材料。它使用项目自行维护的
`.ahclkitconfigs` 配置文件，生成确定性输出，并通过明确的命令下载许可证和收集
依赖许可证证据。

Rust 适配器支持 Cargo 项目。JavaScript 适配器支持 npm、pnpm、Yarn 和 Bun，
并直接读取对应的锁文件。

## 安装

| 系统 | 架构 | 安装包 |
| --- | --- | --- |
| Windows | x64、ARM64 | MSI |
| macOS 11 或更高版本 | Intel、Apple Silicon | 通用 PKG |
| Debian/Ubuntu Linux | x86-64、ARM64 | DEB |
| 使用 RPM 的 Linux | x86-64、ARM64 | RPM |

从 GitHub Releases 下载对应系统的安装包，并使用平台的软件包管理器安装。安装完成后，
打开新的终端并运行：

```console
ahcl --version
```

## 快速开始

```console
ahcl config init
```

编辑 `.ahclkitconfigs`，然后初始化 AHCL Materials Directory 和所选许可证版本：

```console
ahcl project init
ahcl project generate
ahcl project generate --dry-run
ahcl project check
```

未提供项目路径时，命令默认使用当前目录。普通命令会向上查找
`.ahclkitconfigs`，同时支持显式项目路径和用于批处理的项目列表文件。

## 命令

| 命令 | 用途 |
| --- | --- |
| `ahcl config init` | 创建结构完整且未选择语言的配置文件。 |
| `ahcl config validate` | 验证项目配置。 |
| `ahcl config show` | 显示应用默认值后的配置。 |
| `ahcl project init` | 初始化配置、许可证和 AHCL Materials Directory。 |
| `ahcl project generate` | 生成所有已配置的项目材料。 |
| `ahcl project check` | 在不修改文件的情况下报告偏差。 |
| `ahcl license sync` | 下载并验证所选的官方 AHCL 许可证。 |
| `ahcl dependency generate` | 生成 `[materials-directory]/AHCL-DEPENDENCIES.md`。 |
| `ahcl third-party generate` | 在 `[materials-directory]/THIRD-PARTY-LICENSES/` 下生成逐依赖许可证证据。 |

写入类命令支持 `--dry-run`。使用 `ahcl <command> --help` 查看可用参数和输出格式。

## 配置

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
enabled = true
covered-scope = "示例程序"
special-authorization-channel = ""

[generation]
strict-license-files = true

[rust.cargo]
manifests:
  - "Cargo.toml"
packages = []
rules = []

[rust.cargo.evidence.example]
package = "example-crate"
version = "1.0.0"
source = "git+https://example.com/example-crate"
repository = "https://example.com/example-crate"
revision = "0123456789abcdef0123456789abcdef01234567"
path = "LICENSE"
url = "https://example.com/example-crate/LICENSE"
kind = "license"

[rust.cargo.component.example]
package = "example-crate"
enabled = true
layout = "independent"
materials-directory = ".ahcl"
license-version = "1.2"
covered-scope = "示例组件"
right-holders:
  - "Example Foundation"
canonical-repository = "https://example.com/example.git"
canonical-branch = "main"
contact = ""
adoption-date = "2026-09-20"
special-authorization-channel = ""
```

```text
[javascript]
manifests:
  - "package.json"
managers:
  - "npm"
packages = []
rules = []
```

`managers` 可取 `npm`、`pnpm`、`yarn` 和 `bun`。省略 `managers` 时，使用每个
清单旁边唯一的锁文件：`package-lock.json`、`pnpm-lock.yaml`、`yarn.lock` 或
文本形式的 `bun.lock`。npm 锁文件须为版本 2 或 3，pnpm 锁文件须为版本 6 或 9。
Yarn classic 与 Yarn Berry 均可读取。二进制 `bun.lockb` 会被拒绝。JavaScript
适配器不会安装依赖、遍历 `node_modules` 或访问注册表。存在多个锁文件时，必须
显式设置 `managers`。

`enabled`、`covered-scope`、`[rust.cargo.evidence.*]` 和
`[rust.cargo.component.*]` 是可选项。证据 `kind` 为 `license`、`notice` 或
`materials`。组件 `layout` 为 `independent` 或 `centralized`。省略 `schema`
时使用最新支持的 schema；省略 `materials-directory` 时使用
`.ahcl`；省略语言时解析为空集合。语言集合为空时仍可执行通用 AHCL 项目材料命令，
但不会调用任何生态适配器。

## 从源码构建

```console
cargo build --locked --release --package ahcl-kit --bin ahcl
```

## 许可证

AHCL Kit 使用 AHCL 1.1。使用、修改或分发项目前，请阅读 [LICENSE](../LICENSE)和完整的
[AHCL Materials Directory](../.ahcl/)。

</div>
