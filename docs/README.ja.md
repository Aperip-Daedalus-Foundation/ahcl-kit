<div align="center">

<p>
  <img src="../assets/branding/ahcl-kit.svg" width="144" height="144" alt="AHCL Kit アイコン">
</p>

# AHCL Kit

<p>
  <img src="https://img.shields.io/github/v/release/Aperip-Daedalus-Foundation/ahcl-kit?display_name=tag&amp;style=flat-square&amp;label=release" alt="最新リリース">
  <img src="https://img.shields.io/github/stars/Aperip-Daedalus-Foundation/ahcl-kit?style=flat-square&amp;label=stars" alt="リポジトリの Star 数">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-334155?style=flat-square" alt="対応プラットフォーム">
  <img src="https://img.shields.io/badge/license-AHCL%201.1-0F766E?style=flat-square" alt="AHCL 1.1 ライセンス">
</p>

[English](../README.md) | [简体中文](README.zh-CN.md) | 日本語

AHCL Kit は、プロジェクト向けの AHCL マテリアルを初期化、生成、保守するための
ツールです。プロジェクトが所有する `.ahclkitconfigs` を使用し、決定的な出力を
生成します。ライセンスの取得と依存関係のライセンス証拠収集は、明示的なコマンドで
実行されます。

現在のアダプターは、Cargo で管理される Rust プロジェクトのみをサポートしています。
ほかのパッケージマネージャーは今後サポートする予定です。

## インストール

| システム | アーキテクチャ | パッケージ |
| --- | --- | --- |
| Windows | x64、ARM64 | MSI |
| macOS 11 以降 | Intel、Apple Silicon | Universal PKG |
| Debian/Ubuntu Linux | x86-64、ARM64 | DEB |
| RPM ベースの Linux | x86-64、ARM64 | RPM |

GitHub Releases から対象システムのパッケージをダウンロードし、各プラットフォームの
パッケージマネージャーでインストールしてください。インストール後、新しいターミナルを
開いて次を実行します。

```console
ahcl --version
```

## クイックスタート

```console
ahcl config init
```

`.ahclkitconfigs` を編集し、AHCL Materials Directory と選択したライセンス
バージョンを初期化します。

```console
ahcl project init
ahcl project generate
ahcl project generate --dry-run
ahcl project check
```

プロジェクトパスを指定しない場合、コマンドは現在のディレクトリを使用します。
通常のコマンドは上位ディレクトリから `.ahclkitconfigs` を検出します。また、明示的な
プロジェクトパスと一括処理用のプロジェクト一覧ファイルにも対応します。

## コマンド

| コマンド | 用途 |
| --- | --- |
| `ahcl config init` | 言語未選択の完全な設定ひな形を作成します。 |
| `ahcl config validate` | プロジェクト設定を検証します。 |
| `ahcl config show` | 既定値を適用した設定を表示します。 |
| `ahcl project init` | 設定、ライセンス、AHCL Materials Directory を初期化します。 |
| `ahcl project generate` | 設定されたすべてのプロジェクトマテリアルを生成します。 |
| `ahcl project check` | ファイルを変更せずに差分を報告します。 |
| `ahcl license sync` | 選択した公式 AHCL ライセンスを取得して検証します。 |
| `ahcl dependency generate` | `[materials-directory]/AHCL-DEPENDENCIES.md` を生成します。 |
| `ahcl third-party generate` | `[materials-directory]/THIRD-PARTY-LICENSES/` に依存項目ごとのライセンス証拠を生成します。 |

書き込みを行うコマンドは `--dry-run` に対応します。使用できるオプションと出力形式は
`ahcl <command> --help` で確認できます。

## 設定

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
covered-scope = "サンプルプログラム"
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
covered-scope = "サンプルコンポーネント"
right-holders:
  - "Example Foundation"
canonical-repository = "https://example.com/example.git"
canonical-branch = "main"
contact = ""
adoption-date = "2026-09-20"
special-authorization-channel = ""
```

`enabled`、`covered-scope`、`[rust.cargo.evidence.*]`、`[rust.cargo.component.*]`
は任意です。証拠の `kind` は `license`、`notice`、`materials` です。コンポーネントの
`layout` は `independent` または `centralized` です。`schema` を省略すると最新の対応スキーマ、`materials-directory` を省略すると
`.ahcl` が使用されます。言語を省略すると空の集合になります。言語が空でも一般的な
AHCL プロジェクトマテリアルのコマンドは実行できますが、エコシステムアダプターは
呼び出されません。

## ソースからのビルド

```console
cargo build --locked --release --package ahcl-kit-cli --bin ahcl
```

## ライセンス

AHCL Kit は AHCL 1.1 を使用しています。利用、変更、配布の前に
[LICENSE](../LICENSE) と完全な [AHCL Materials Directory](../.ahcl/) を確認してください。

</div>
