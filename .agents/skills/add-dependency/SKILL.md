---
name: add-dependency
description: Add a new external crate dependency to the workspace or a specific crate. Covers workspace Cargo.toml, deny.toml, and verification.
license: MIT
compatibility: opencode
---

## Steps

1. **Add to workspace** `Cargo.toml`:
   - Add the dependency to `[workspace.dependencies]` with appropriate version/features
   - Keep entries alphabetically sorted
   - Use `features = [...]` explicitly when specific features are needed
2. **Add to specific crate's** `Cargo.toml`:
   - Use `crate_name.workspace = true` if it's a workspace dependency
   - Or add inline if it's only used by one crate
3. **For dev-only or test-only deps**, add under `[dev-dependencies]` in the specific crate
4. **Build**: `cargo build` to verify it compiles
5. **License check**: `cargo deny --workspace check`
   - If deny fails on license, update `deny.toml`:
     - Add license to `[licenses.allowed]` if it's a known OSI-approved license
     - Add package to `[licenses.deny]` never — prefer `skip` if unavoidable
   - If deny fails on duplicate, add to `[bans.skip]` if the duplicate is intentional
6. **Test**: `cargo test -p <affected-crate>` then `make lint`

## When to use

Use when asked to add a new Rust crate dependency to the project, either at the workspace level or for a specific crate.
