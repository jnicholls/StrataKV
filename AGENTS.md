# AGENTS.md

## Project overview

**MentatKV** is an embedded key-value storage engine library written in Rust. It is a single-crate Rust library (`mentatkv`) with no external service dependencies.

## Cursor Cloud specific instructions

### Toolchain

- Rust stable (latest) is required. The VM comes with `rustup`; run `rustup update stable` if the toolchain is stale.
- A C/C++ toolchain (`gcc`, `g++`) and system linker (`ld`) must be present for crates that use C bindings. These are pre-installed on the Cloud VM (Ubuntu 24.04).

### Key commands

| Task | Command |
|------|---------|
| Build | `cargo build` |
| Test | `cargo test` |
| Lint | `cargo clippy -- -D warnings` |
| Format check | `cargo fmt -- --check` |
| Auto-format | `cargo fmt` |
| Run example | `cargo run --example hello` |

### Notes

- This is a pure library crate; there is no long-running server to start. Development verification is done via `cargo test` and example binaries under `examples/`.
- The `edition = "2024"` in `Cargo.toml` requires Rust 1.85+. Ensure the toolchain is up to date before building.
