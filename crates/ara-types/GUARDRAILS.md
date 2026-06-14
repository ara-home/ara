# GUARDRAILS.md — ara-types

---

## SIGN #1: Internal Dependency
**Trigger:** Adding a dependency on another ara crate (e.g., `ara-util`, `ara-store`)
**Instruction:** Do NOT add internal ara dependencies. `ara-types` must remain the root of the DAG with zero internal deps.
**Reason:** Every crate in the workspace depends on `ara-types`. Adding an internal dep would create a cyclic dependency or break the layered architecture.
**Provenance:** Architectural invariant. Root of DAG.

---

## SIGN #2: Async Introduction
**Trigger:** Adding `async`, `tokio`, `futures`, or any async runtime dependency
**Instruction:** Keep `ara-types` synchronous. It contains pure domain types and parsing logic only.
**Reason:** Async would force all downstream consumers (every crate) into an async context, violating the zero-overhead abstraction principle.
**Provenance:** Architectural decision.

---

## SIGN #3: Unsafe
**Trigger:** Adding `unsafe` to any part of `ara-types`
**Instruction:** Do NOT use `unsafe`. All domain types can be expressed in safe Rust.
**Reason:** `ara-types` contains only data structures, enums, and pure functions. There is no FFI, no raw pointer manipulation, and no performance-sensitive hot path that requires `unsafe`.
**Provenance:** Project-wide invariant: unsafe only in `ara-sandbox`.

---

## SIGN #4: Public API Removal
**Trigger:** Removing a `pub` type, function, or re-export (e.g., `SourceType`, `Constraint`, `RiskLevel`, `PackageIdentity`, `semver::Version`)
**Instruction:** Before removing or renaming any public API, check all reverse dependencies across the workspace. Deprecate before removing.
**Reason:** `ara-types` is the shared language of the entire project. Removing public types breaks every crate and causes widespread compilation failures.
**Provenance:** API surface defined by usage across all 9 dependent crates.
