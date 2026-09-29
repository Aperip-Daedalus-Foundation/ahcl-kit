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
cargo build-workspace
cargo test-all
```

The CLI should continue to build on Windows, macOS, and Linux. Filesystem code
must retain platform-specific handle and link protections rather than replacing
them with ambient path operations.

## Required Checks

Before requesting review, run:

```console
cargo fmt-check
cargo check-all
cargo clippy-all
cargo test-all
cargo build-ahcl
target/debug/ahcl project generate --dry-run
target/debug/ahcl project check
```

On Windows, use `target\debug\ahcl.exe` for the final two commands.

## Source Limits

A handwritten production function, including a closure, stays at McCabe
cyclomatic complexity 8 or below. Count branches, loops, short-circuit
boolean arms, `match` arms, and early returns. Splitting an expression across
lines does not reduce the count.

Keep a function within 80 lines, 4 levels of nesting, and 6 parameters. Split
it when it passes one of those limits, unless the split would hide a safety
or protocol boundary. Explain that case in the pull request.

Review a source file as it nears 800 lines. Split it before 1,200 lines in
the same change, or explain why it cannot be split. Do not count the AHCL
notice at the top of the file.

## Crate Boundaries

An extension crate implements one language or package manager and depends on
the public crates. A public crate does not depend on, name, call, or contain
an extension.

The public crates are `ahcl-kit-core`, `ahcl-kit-fs`, `ahcl-kit-config`,
`ahcl-kit-license`, `ahcl-kit-materials`, and `ahcl-kit-cli`. They expose
language-neutral contracts only. Ecosystem identifiers, settings, lockfile
parsing, and other package-manager behavior stay in the extension that owns
them.

`crates/ahcl-kit` is the composition root and the only crate that depends on
extensions. It registers their contributors and hosts. `assets/packaging`
builds that binary and does not name an ecosystem. Adding a language or
package manager means a new extension crate and a registration in the
composition root. Do not change a public crate to teach it the new ecosystem.

Behavior used by more than one extension belongs in a public crate. Do not
copy it into an extension. Behavior used by only one ecosystem stays there.

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

Use Conventional Commit subjects such as `feat(config):`, `fix(packaging):`,
`docs(license):`, `refactor(cli):`, and `chore(release):`. Split commits by
responsibility. Do not combine unrelated work into one commit that is
difficult to review. Include a body when the reason or compatibility impact
is not obvious.

Pull requests should explain the user-visible behavior, compatibility impact,
security considerations, and commands used for verification. Do not include
unrelated generated files or development-only artifacts.
