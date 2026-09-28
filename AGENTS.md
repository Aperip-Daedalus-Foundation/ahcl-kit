# AHCL Kit

AHCL Kit initializes, generates, and maintains AHCL materials for projects.

## Public Crates

- `ahcl-kit-core`: shared domain boundaries for generators and adapters.
- `ahcl-kit-fs`: no-follow reads of package evidence.
- `ahcl-kit-config`: strict parsing and resolution of `.ahclkitconfigs`.
- `ahcl-kit-license`: download and verification of official AHCL license records.
- `ahcl-kit-materials`: deterministic planning and capability-scoped application of AHCL materials.
- `ahcl-kit-cli`: command parsing, project discovery, batch execution, and output.

## Composition Root

`ahcl-kit` is the executable crate. It is the only crate that depends on
language extensions, and it registers them. It does not implement an ecosystem
itself.

The other crates in `crates/` are language extensions and are not listed here.

## Before Starting

Read [CONTRIBUTING.md](CONTRIBUTING.md) and follow its requirements.
