[![continuous-integration](https://github.com/killzoner/cargo-neat/actions/workflows/continuous-integration.yml/badge.svg)](https://github.com/killzoner/cargo-neat/actions/workflows/continuous-integration.yml)

# cargo-neat

> **Keep your cargo workspace neat**

## About

A command to complement existing tools like [cargo-machete](https://github.com/bnjbvr/cargo-machete) when working with a Cargo workspace.

Features:

- detect unused dependencies in `workspace.dependencies` when working with a cargo workspace
- optionally enforce using only workspace dependency in your project (`-m` option)

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
cargo neat -m my-workspace

Unused workspace dependencies :
└── /home/user/my-workspace/Cargo.toml
    ├── anyhow
    └── clappen

Non workspace dependencies :
├── /home/user/my-workspace/crate1/Cargo.toml
│   ├── futures-lite
│   └── argh
└── /home/user/my-workspace/crate2/Cargo.toml
    └── clap
```

The **return code** gives an indication whether unused dependencies have been found:

- 0 if it found no unused dependencies,
- 1 if it found at least one unused dependency,
- 2 if there was an error during processing (in which case there's no indication whether any unused
  dependency was found or not).

## Inspiration

A lot of the code structure is drawn from the great [cargo-machete](https://github.com/bnjbvr/cargo-machete).
If you don't already use it, you probably should.
