<div align="center">

<p>
  <img src="../assets/branding/ahcl-kit.svg" width="144" height="144" alt="AHCL Kit アイコン">
</p>

# AHCL Kit

<p>
  <img src="https://img.shields.io/github/v/release/Aperip-Daedalus-Foundation/ahcl-kit?display_name=tag&amp;style=flat-square&amp;label=release" alt="最新リリース">
  <img src="https://img.shields.io/github/stars/Aperip-Daedalus-Foundation/ahcl-kit?style=flat-square&amp;label=stars" alt="リポジトリの Star 数">
  <img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-334155?style=flat-square" alt="対応プラットフォーム">
  <img src="https://img.shields.io/badge/license-AHCL%201.2-0F766E?style=flat-square" alt="AHCL 1.2 ライセンス">
</p>

[English](../README.md) | [简体中文](README.zh-CN.md) | 日本語

AHCL Kit は、プロジェクト向けの AHCL マテリアルを初期化、生成、保守するための
ツールです。プロジェクトが所有する `.ahclkitconfigs` を使用し、決定的な出力を
生成します。ライセンスの取得と依存関係のライセンス証拠収集は、明示的なコマンドで
実行されます。

Rust アダプターは Cargo プロジェクトをサポートします。JavaScript アダプターは
npm、pnpm、Yarn、Bun をサポートし、それぞれのロックファイルを直接読み取ります。

## インストール

リリースには、次のシステム向けのネイティブパッケージが含まれます。

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

現在のプロジェクトに完全な設定ひな形を作成します。

```console
ahcl config init
```

`.ahclkitconfigs` を編集し、AHCL Materials Directory と選択したライセンス
バージョンを初期化します。

```console
ahcl project init
```

設定されたすべてのマテリアルを生成します。

```console
ahcl project generate
```

ファイルに書き込まずに変更内容を確認します。

```console
ahcl project generate --dry-run
```

生成されたファイルが最新かどうかを確認します。

```console
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
| `ahcl where` | この実行ファイルの絶対パスを表示します。 |

書き込みを行うコマンドは `--dry-run` に対応します。使用できるオプションと出力形式は
`ahcl <command> --help` で確認できます。

## 設定

</div>

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
```

```text
[rust.cargo]
manifests:
  - "Cargo.toml"
packages = []
rules = []

[rust.cargo.evidence.example]
package = "example-crate"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
repository = "https://github.com/example/example-crate"
revision = "0123456789abcdef0123456789abcdef01234567"
path = "LICENSE"
url = "https://raw.githubusercontent.com/example/example-crate/0123456789abcdef0123456789abcdef01234567/LICENSE"
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

<div align="center">

`enabled`、`covered-scope`、`[rust.cargo.evidence.*]`、`[rust.cargo.component.*]`
は任意です。証拠の `kind` は `license`、`notice`、`materials` です。コンポーネントの
`layout` は `independent` または `centralized` です。`schema` を省略すると最新の対応スキーマ、`materials-directory` を省略すると
`.ahcl` が使用されます。言語を省略すると空の集合になります。言語が空でも一般的な
AHCL プロジェクトマテリアルのコマンドは実行できますが、エコシステムアダプターは
呼び出されません。

`package.license` を直接宣言するサブクレートでも、明示的な `kind = "license"`
マッピングでリポジトリ直下の `LICENSE`、`COPYING`、`COPYRIGHT` を指定できます。
workspace からの継承は必要ありません。依存パッケージの正確なバージョン、取得元、
完全なコミットハッシュと、同じリポジトリ・コミットを指す URL を指定してください。
パッケージのメタデータから、そのコミット内のサブクレートの位置を確認できない
場合は、マッピングを拒否します。
既存の `license-file` 宣言を上書きしたり、他のパッケージのライセンスを流用したり
することはできません。

</div>

```text
[javascript]
manifests:
  - "package.json"
managers:
  - "npm"
packages = []
rules = []
```

<div align="center">

`managers` は `npm`、`pnpm`、`yarn`、`bun` を受け付けます。省略した場合は、各
マニフェストの隣にある単一のロックファイル（`package-lock.json`、
`pnpm-lock.yaml`、`yarn.lock`、またはテキスト形式の `bun.lock`）を使います。
npm のロックファイルはバージョン 2 または 3、pnpm はバージョン 6 または 9 です。
Yarn classic と Yarn Berry の両方を読み取れます。バイナリの `bun.lockb` は拒否
されます。JavaScript アダプターは依存関係のインストール、`node_modules` の走査、
レジストリへの接続を行いません。ロックファイルが複数ある場合は `managers` を
明示してください。

</div>

AHCL 1.2 の対象範囲の説明が長い場合は、リテラルテキストブロックを使用できます。

```text
[license]
version = "1.2"
covered-scope: |
  このプロジェクトが管理する src/ のソースコードとビルドスクリプト、
  およびこれらのファイルに対する変更。

  サードパーティーの依存関係には、それぞれのライセンスが適用されます。
```

本文の各行を半角スペース2個で字下げします。この2個のスペースだけが除かれ、
追加の字下げ、改行、途中の空行は `LICENSE` に保持されます。先頭と末尾の空行は
除かれます。引用符、バックスラッシュ、`#` はエスケープやコメントではなく、
文字どおりに扱われます。次の空でない字下げのない行でブロックが終了します。
従来の `covered-scope = "..."` 形式と `\n` エスケープも引き続き使用できます。
コンポーネントの `covered-scope` にも同じ構文を使用します。

<div align="center">

## ソースからのビルド

Rust ツールチェーンのバージョンはリポジトリで固定されています。次のコマンドで
実行ファイルをビルドします。

```console
cargo build-release
```

## ライセンス

AHCL Kit は AHCL 1.2 を使用しています。利用、変更、配布の前に
[LICENSE](../LICENSE) と完全な [AHCL Materials Directory](../.ahcl/) を確認してください。

</div>
