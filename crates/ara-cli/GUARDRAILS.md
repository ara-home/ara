# GUARDRAILS.md — ara-cli

---

## SIGN #1: Error Handling Pattern
**Trigger:** Replacing `anyhow::Result` with a custom error type for command handlers, or removing `.context()` / `.with_context()` chaining
**Instruction:** Keep `anyhow::Result<()>` for all command handlers (`cmd_install`, `cmd_x`, `cmd_run`, etc.). Use `.context()` or `.with_context()` for error enrichment.
**Reason:** The CLI crate is the top of the error-handling chain. Lower-level errors from all other crates propagate here. `anyhow` provides the flexibility to wrap diverse error types with meaningful context. Changing this would require wrapping every error from every crate.
**Provenance:** Established pattern since initial CLI implementation.

---

## SIGN #2: Version Injection
**Trigger:** Changing how `VERSION` is injected in `version.rs` or modifying the `build.rs` fallback logic
**Instruction:** Keep `env!("ARA_VERSION")` as the primary source, with `env!("CARGO_PKG_VERSION")` as fallback. If changing the injection mechanism, update the cargo-dist CI pipeline in lockstep.
**Reason:** The version is injected at build time for cargo-dist releases. The fallback ensures local development builds have a version string. Breaking this breaks release artifact naming.
**Provenance:** Build-time injection via `build.rs` and CI.

---

## SIGN #3: Command Stubs
**Trigger:** Removing or implementing not-yet-implemented commands (`Build`, `Publish`, `Trust`)
**Instruction:** Keep stubs for unimplemented commands with a clear error message. If implementing, ensure the full install pipeline, sandbox integration, and lockfile generation are included.
**Reason:** Stubs define the CLI contract and roadmap. Removing them changes the user-facing interface. Implementing without full pipeline creates half-baked features.
**Provenance:** Commands enum in `cli/mod.rs` defines Build, Publish, Trust variants.
