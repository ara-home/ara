# GUARDRAILS.md — ara-resolver

---

## SIGN #1: MVS Heuristic
**Trigger:** Changing the version selection algorithm from MVS (minimum version selection) to another strategy (e.g., maximum version, SAT solver)
**Instruction:** Keep MVS as the primary selection strategy. If adding alternative strategies, make them opt-in and never change the default.
**Reason:** MVS guarantees deterministic resolution across machines — the core promise of Ara. Changing the algorithm would break reproducibility and invalidate existing lockfiles.
**Provenance:** Core design principle. Documented in `README.md` and implemented in `mvs.rs`.

---

## SIGN #2: Cycle Detection
**Trigger:** Removing or weakening cycle detection in `Graph::has_cycles()`
**Instruction:** Keep DFS-based cycle detection on every `Graph`. If optimizing, preserve the invariant that `has_cycles()` returns `true` for any cycle (self-loops, 2-node, 3-node+).
**Reason:** Dependency graphs with cycles cannot be topologically sorted and cause infinite loops during install. Cycle detection prevents hard-to-debug runtime failures.
**Provenance:** Implemented in `graph.rs` with tests for all cycle shapes.

---

## SIGN #3: Async Introduction
**Trigger:** Adding `async`, `tokio`, or `futures` as a dependency of `ara-resolver`
**Instruction:** Keep `ara-resolver` synchronous. Network calls and I/O happen in `ara-source` and `ara-cli`. The resolver is a pure computation unit.
**Reason:** The resolver performs constraint matching and graph operations — pure computation that benefits from being synchronous and testable without a runtime.
**Provenance:** Architectural decision. Resolver is a pure dataflow crate.
