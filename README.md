[![continuous-integration](https://github.com/killzoner/cargo-neat/actions/workflows/continuous-integration.yml/badge.svg)](https://github.com/killzoner/cargo-neat/actions/workflows/continuous-integration.yml)

# cargo-neat

> **Keep your cargo workspace neat**

## About

A command to complement existing tools like [cargo-machete](https://github.com/bnjbvr/cargo-machete) when working with a Cargo workspace.

Features:

- detect unused dependencies in `workspace.dependencies` when working with a cargo workspace
- optionally enforce using only workspace dependency in your project (`-m` option)
- optionally enforce using only workspace metadata for some sections in workspace packages (`-p` option)

## Installation

Install with cargo:

`cargo install cargo-neat`

## Usage

```bash
cd my-directory && cargo neat
```

or alternatively

```bash
cargo neat my-directory
```

## Sample output

```bash
cargo neat -m -p my-workspace

Unused workspace dependencies :
└── /home/user/my-workspace/Cargo.toml
    ├── anyhow
    └── clappen

Non workspace dependencies :
├── /home/user/my-workspace/crate1/Cargo.toml
│   ├── argh
│   └── futures-lite
└── /home/user/my-workspace/crate2/Cargo.toml
    └── clap

Non workspace metadata :
├── /home/user/my-workspace/crate1/Cargo.toml
    └── edition
```

The **return code** gives an indication whether unused dependencies have been found:

- 0 if it found no unused dependencies,
- 1 if it found at least one unused dependency,
- 2 if there was an error during processing (in which case there's no indication whether any unused
  dependency was found or not).

## Inspiration

A lot of the code structure is drawn from the great [cargo-machete](https://github.com/bnjbvr/cargo-machete).
If you don't already use it, you probably should.

## Similar tools

- [est31/cargo-udeps](https://github.com/est31/cargo-udeps): slow and requires Nightly rust, has no workspace features
- [bnjbvr/cargo-machete](https://github.com/bnjbvr/cargo-machete): fast but does not remove unused workspace dependencies
- [Boshen/cargo-shear](https://github.com/Boshen/cargo-shear): removes unused workspace dependencies, and can fix wrong dependency section, but cannot enforce using workspace dependencies only

I would need a mix of `cargo-machete/shear` and `cargo-neat`. However `cargo-shear` is marked feature complete and not accepting new features, so here is `cargo-neat`.
