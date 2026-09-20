# Contributing to AHCL Kit

Contributions should preserve deterministic generation, explicit capability
boundaries, cross-platform behavior, and the static adapter architecture.

## Before Starting

1. Read [LICENSE](LICENSE) and the notices in [.ahcl/](.ahcl/).
2. Open an issue for changes that alter configuration syntax, generated file
   layout, adapter contracts, or release packaging.
3. Keep changes scoped. Do not combine feature work with unrelated refactoring.

By submitting a contribution, you represent that you have the right to provide
it under the project's applicable AHCL terms and notices.

## Development Setup

Install the Rust toolchain selected by `rust-toolchain.toml`, then run:

```console
cargo build --locked --workspace
cargo test --locked --workspace --all-targets
```

The CLI should continue to build on Windows, macOS, and Linux. Filesystem code
must retain platform-specific handle and link protections rather than replacing
them with ambient path operations.

## Required Checks

Before requesting review, run:

```console
cargo fmt --all -- --check
cargo check --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets
cargo build --locked --package ahcl-kit-cli --bin ahcl
target/debug/ahcl project generate --dry-run
target/debug/ahcl project check
```

On Windows, use `target\debug\ahcl.exe` for the final two commands.

## Tests

Verify behavior changes with focused, repeatable checks. Security-sensitive
filesystem, configuration, license transport, dependency evidence, and
generation changes must cover failure cases as well as successful output.
Verification must not depend on public network availability or mutate a
developer's working project.

Do not submit implementation plans, internal or non-public documents,
temporary files, local test harnesses, one-off verification code, generated
scratch data, editor state, tool caches, or other development-only artifacts.
Only files intended to form part of the maintained public project belong in a
pull request.

## Generated AHCL Materials

Do not edit generated dependency or third-party license output by hand. After a
dependency change, rebuild `ahcl` and regenerate the repository materials with
the corresponding `dependency generate`, `third-party generate`, or
`project generate` command. Review the resulting evidence before including it.

## Product Metadata And Releases

The root `Cargo.toml` is the only source for the product version and shared
application metadata. Member crates inherit the workspace version. Do not copy
version strings into scripts, workflows, installer definitions, or platform
metadata templates.

A release is initiated when the version in the root manifest changes on the
default branch and the corresponding `v<version>` tag does not exist. Packaging
changes should preserve Windows x64/ARM64, macOS Universal 2, and Linux
x86-64/ARM64 output.

## Commits And Pull Requests

Use Conventional Commit subjects such as `feat:`, `fix:`, `docs:`, `test:`,
`refactor:`, and `chore:`. Keep commits independently reviewable and include a
body when the reason or compatibility impact is not obvious.

Pull requests should explain the user-visible behavior, compatibility impact,
security considerations, and commands used for verification. Do not include
unrelated generated files or development-only artifacts.
