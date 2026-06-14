# Ara

A dependency manager for JS/TS with built-in security analysis. Written in Rust. Cargo workspace with 10 crates.

## Build & Test

- **Build all:** `cargo build`
- **Build release:** `cargo build --release`
- **Run all tests:** `make test-all` (unit + e2e + fixtures)
- **Unit tests only:** `cargo test --lib --quiet`
- **Single crate:** `cargo test -p ara-<crate>`
- **Fixture tests:** `cargo test -p ara-cli --test fixture_test`
- **Lint:** `make lint` (clippy + fmt check)
- **Pedantic:** `make lint-pedantic`
- **Full CI:** `make ci` (lint + audit + deny + test)
- **Audit:** `cargo audit`
- **License/duplicate check:** `cargo deny --workspace check`
- **Benchmarks:** `cargo bench -p ara-cli`

## Project Structure

```
ara-types/     # Domain types: SourceType, Constraint, RiskLevel, Finding, PackageIdentity
ara-util/      # SHA-256 hashing, HTTP client with retry + TLS
ara-store/     # Content-addressable store (CAS sharded fs + SQLite index)
ara-source/    # Package fetchers: npm registry, GitHub, git, tarball, local, workspace
ara-analysis/  # Security scanner: 17 regex patterns (eval, creds, obfuscation, etc.)
ara-manifest/  # Parses package.json and ara.toml
ara-lockfile/  # Reads/writes ara.lock (TOML, no serde — manual serialization)
ara-resolver/  # MVS resolver with graph cycle detection
ara-sandbox/   # Seccomp-BPF executor (hermetic/restricted/open profiles)
ara-cli/       # Binary entrypoint + clap command dispatch
```

## Architecture

- **DAG dependency graph**: `ara-types` is the root (zero internal deps). `ara-cli` depends on everything. No circular dependencies allowed.
- **Error handling**: `thiserror` for library crates, `anyhow` (with `.context()`) in CLI and analysis.
- **Async**: tokio multi-thread (`rt-multi-thread` + `macros`). No async traits — dispatch via Source enum.
- **unsafe**: Only in `ara-sandbox` (seccomp-BPF via `libc::prctl`). Zero unsafe anywhere else.
- **Module visibility**: `pub mod` in lib.rs. `pub(crate)` for internal functions. Minimal re-exports (only `workspace` alias and `semver::Version`).

## Code Style

- `snake_case` for functions, variables, modules
- `CamelCase` for types, enum variants
- `SCREAMING_CASE` for constants
- `#[must_use]` on public functions returning values
- `#[allow(clippy::unwrap_used)]` only inside `#[cfg(test)]` blocks
- Use `.context()` / `.with_context()` from anyhow — never `unwrap()`/`expect()` in production code
- Prefer `map_err` + `?` over manual error matching
- Crate-level `#![deny(...)]` or CI-level clippy denies: `unwrap_used`, `expect_used`, `panic`

## Commits

Conventional Commits: `type(scope): message`

- Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`
- Scopes: `types`, `cli`, `source`, `resolver`, `sandbox`, `store`, `analysis`, `manifest`, `lockfile`, `util`
- English, imperative mood, short summary
- Example: `feat(resolver): add cycle detection to dependency graph`

## Testing

- **Unit**: inline `#[cfg(test)] mod tests` in each module
- **Integration**: fixture-based in `ara-cli/tests/fixtures/` with mockito HTTP mock
- **Benchmarks**: `ara-cli/benches/` via codspeed-criterion-compat
- Always use `tempfile::TempDir` for filesystem tests
- Use `#[tokio::test]` for async tests
- Suppress clippy with `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` in test modules

## Boundaries

- ✅ Run `make lint` before every commit
- ✅ Run `make test-all` to verify no regressions
- ✅ Use `tempfile::TempDir` for any test touching the filesystem
- ⚠️ Adding new dependencies to `Cargo.toml` — ask first
- ⚠️ Modifying sandbox syscall tables (x86_64 specific, breaks other archs)
- ⚠️ Changing the lockfile hash format (`sha256-<64hex>`) — requires migration
- 🚫 **Never** add `unsafe` outside `ara-sandbox`
- 🚫 **Never** introduce circular crate dependencies
- 🚫 **Never** commit secrets, tokens, or credentials
- 🚫 **Never** modify `deny.toml` without understanding the implications

## Guardrails

You MUST read and follow `GUARDRAILS.md` (root) and the per-crate `GUARDRAILS.md` before making changes. Each Sign documents a failure pattern with Trigger → Instruction → Reason → Provenance:

- `GUARDRAILS.md` — 8 cross-cutting Signs (unsafe, circular deps, deny.toml, lockfile format, workspace deps, clippy, async traits, CI/CD)
- `crates/ara-types/GUARDRAILS.md` — internal deps, async, unsafe, public API removal
- `crates/ara-util/GUARDRAILS.md` — hash algorithm, HTTPS enforcement, retry logic
- `crates/ara-store/GUARDRAILS.md` — sharded layout migration, integrity verification, key validation
- `crates/ara-source/GUARDRAILS.md` — enum dispatch, URL validation, cache format
- `crates/ara-analysis/GUARDRAILS.md` — anyhow usage, deduplication key, file scan limits
- `crates/ara-manifest/GUARDRAILS.md` — round-trip fidelity, name validation, workspace protocol
- `crates/ara-lockfile/GUARDRAILS.md` — serialization approach, hash format, version constraint
- `crates/ara-resolver/GUARDRAILS.md` — MVS heuristic, cycle detection, async introduction
- `crates/ara-sandbox/GUARDRAILS.md` — unsafe proliferation, arch-specific syscalls, hermetic profile
- `crates/ara-cli/GUARDRAILS.md` — error handling, version injection, command stubs

## Per-Crate Guides

Each crate has its own AGENTS.md with crate-specific API, conventions, and test commands. Read the relevant one when working in that crate:

- `crates/ara-types/AGENTS.md` — domain types, root of DAG
- `crates/ara-util/AGENTS.md` — hashing + HTTP client
- `crates/ara-store/AGENTS.md` — content-addressable store + SQLite index
- `crates/ara-source/AGENTS.md` — package fetchers (npm, git, GitHub, tarball, local)
- `crates/ara-analysis/AGENTS.md` — security scanner (17 patterns)
- `crates/ara-manifest/AGENTS.md` — package.json + ara.toml parsing
- `crates/ara-lockfile/AGENTS.md` — lockfile generation + parsing
- `crates/ara-resolver/AGENTS.md` — MVS resolver + cycle detection
- `crates/ara-sandbox/AGENTS.md` — seccomp-BPF sandbox (only unsafe)
- `crates/ara-cli/AGENTS.md` — binary entrypoint + command dispatch

## Available Skills

Loadable on-demand via `skill({ name: "<skill>" })` for task-specific workflows:

- `new-crate` — scaffold a new workspace crate (Cargo.toml, lib.rs, AGENTS.md, registration)
- `add-security-pattern` — add a new security detection pattern to ara-analysis
- `fixture-test` — create fixture-based integration tests with mockito
- `add-dependency` — add an external Rust dependency (workspace Cargo.toml + deny.toml)
- `sandbox-edit` — modify the seccomp-BPF sandbox (high risk, only unsafe crate)
