# GUARDRAILS.md

Cross-cutting constraints for the Ara workspace. Each Sign documents a failure pattern or invariant that must never be violated. Read this file and the relevant per-crate `GUARDRAILS.md` before making changes.

**Total Signs:** 8

---

## SIGN #1: Unsafe Outside Sandbox
**Trigger:** Adding `unsafe` keyword to any crate other than `ara-sandbox`
**Instruction:** Move the code to `ara-sandbox` or refactor to avoid `unsafe`. If neither is possible, escalate for human approval with a written justification.
**Reason:** `ara-sandbox` is the only crate permitted to use `unsafe` (for seccomp-BPF via `libc::prctl`). Allowing `unsafe` elsewhere breaks the project's safety invariant and bypasses the single-responsibility principle for unsafe code.
**Provenance:** Architectural invariant, documented since initial design.

---

## SIGN #2: Circular Crate Dependency
**Trigger:** Adding a dependency from a lower-level crate to a higher-level crate (e.g., `ara-types` depending on `ara-util`, or `ara-store` depending on `ara-cli`)
**Instruction:** Verify no cycles are introduced. If a cycle is detected, refactor to extract shared types into a lower-level crate or use dependency inversion.
**Reason:** The crate DAG must remain acyclic. Circular dependencies cause compilation issues, tight coupling, and violate the layered architecture.
**Provenance:** Architectural invariant. DAG defined in root `AGENTS.md`.

---

## SIGN #3: Deny.Toml Modification
**Trigger:** Being asked to modify `deny.toml` (adding license exceptions, skip entries, or changing bans configuration)
**Instruction:** STOP. Do NOT modify `deny.toml` without explicit human approval. If a new dependency requires a license exception, present the license to the user and ask for a decision.
**Reason:** `deny.toml` controls license auditing and vulnerability checking. Incorrect changes can silently allow incompatible licenses or suppress important security warnings.
**Provenance:** Security policy, enforced from project inception.

---

## SIGN #4: Lockfile Hash Format
**Trigger:** Modifying the lockfile hash format validation or generation in `ara-lockfile`
**Instruction:** Preserve the `sha256-<64hex>` format. If a new format is truly needed, implement a migration path that reads both old and new formats for at least one major version cycle.
**Reason:** The lockfile is committed to repositories and shared across machines. Changing the hash format without migration breaks every existing `ara.lock` file in the ecosystem.
**Provenance:** Format locked at version 1. No migration path exists yet.

---

## SIGN #5: Workspace Dependency Addition
**Trigger:** Adding a new dependency to `[workspace.dependencies]` in the root `Cargo.toml`
**Instruction:** After adding the dependency, run `cargo build` and `cargo deny --workspace check`. If `cargo deny` fails on license, do NOT modify `deny.toml` — ask the user first.
**Reason:** New dependencies can introduce license incompatibilities, duplicate versions, or security vulnerabilities. `cargo-deny` catches these before they reach production.
**Provenance:** CI enforcement. Pipeline defined in `.github/workflows/ci.yml`.

---

## SIGN #6: Clippy Lint Suppression
**Trigger:** Adding `#[allow(clippy::...)]` or `#[deny(clippy::...)]` outside `#[cfg(test)]` modules
**Instruction:** Before suppressing a clippy lint, consider refactoring the code to satisfy it. If suppression is unavoidable, add a comment explaining why. Never suppress `unwrap_used`, `expect_used`, or `panic` in production code.
**Reason:** Clippy lints catch real bugs and enforce consistency. The CI pipeline denies `unwrap_used`, `expect_used`, and `panic` across the workspace. Suppressing these hides errors.
**Provenance:** CI clippy configuration in `.github/workflows/ci.yml`.

---

## SIGN #7: Async Trait Introduction
**Trigger:** Replacing enum-based dispatch (e.g., `Source` enum in `ara-source`) with `#[async_trait]` or trait objects for async dispatch
**Instruction:** Keep enum dispatch. If async traits are necessary, add them alongside the existing enum dispatch and justify the decision in code review.
**Reason:** Async traits add runtime overhead (trait objects, vtable lookups) and complexity (object safety, lifetime issues). The project's enum dispatch pattern works well, keeps async bounds simple, and avoids `Box<dyn>` allocations.
**Provenance:** Architectural decision documented in root `AGENTS.md`.

---

## SIGN #8: CI/CD Configuration Changes
**Trigger:** Modifying files in `.github/workflows/`, `cliff.toml`, `deny.toml`, `dist-workspace.toml`, or the release pipeline
**Instruction:** STOP. Do NOT modify CI/CD configuration files without explicit human approval. Present the proposed changes and the reason for them before proceeding.
**Reason:** CI/CD files control the project's build, test, audit, and release pipeline. Incorrect changes can break releases, skip security audits, or introduce supply-chain risks.
**Provenance:** Operational safety policy. Release pipeline uses cargo-dist with generated config.
