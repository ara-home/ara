---
name: new-crate
description: Scaffold a new workspace crate with Cargo.toml, lib.rs, AGENTS.md, and register it in the workspace. Use when adding a new crate to the Ara workspace at crates/ara-<name>.
license: MIT
compatibility: opencode
---

## Steps

1. **Create directory**: `crates/ara-<name>/src/`
2. **Create `Cargo.toml`**:
   - Use `edition = "2021"`, `license.workspace = true`, `publish = false`
   - Set `[package] name = "ara-<name>"`
   - Add internal deps based on what the crate needs (check DAG order: ara-types -> ara-util -> {store,source,analysis,manifest,lockfile,resolver} -> sandbox -> cli)
   - Add external deps via `workspace = true` when available
3. **Create `src/lib.rs`** with `pub mod` declarations matching the crate's modules
4. **Register in workspace**: add `"crates/ara-<name>"` to `[workspace.members]` in root `Cargo.toml` (keep alphabetical order)
5. **Create `crates/ara-<name>/AGENTS.md`** following the pattern: purpose, public API, conventions, test command, dependencies
6. **Update root `AGENTS.md`**: add crate entry to Project Structure and Per-Crate Guides sections
7. **Verify**: `cargo build` then `cargo test -p ara-<name>`

## When to use

Use this when asked to create a new crate in the Ara workspace, or when a feature needs a new isolated subsystem.
