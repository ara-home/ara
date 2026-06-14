# ara-resolver

MVS (Minimum Version Selection) dependency resolver with graph cycle detection.

## Modules & Public API

**`graph.rs`**:
- `Graph { nodes }` — `Graph::new()`, `add_node(node)`, `find_node(name) -> Option<usize>`
- `compute_hash() -> Result<[u8; 32]>` — SHA-256 of serialized JSON nodes
- `has_cycles() -> bool` — DFS-based cycle detection

**`mvs.rs`**:
- `ConstraintEntry { package, constraint, source, required_by }`
- `Resolver` — `Resolver::new()`, `add_constraint(entry)`, `resolve() -> Graph`
- MVS heuristic: picks the minimum version that satisfies all constraints for each package

## Conventions

- No `async`, no `unsafe`, no `anyhow` (no custom error types)
- MVS is intentionally simple — not a full SAT solver
- `Constraint::LessOrEqual`/`LessThan`/`And` constraints are skipped in candidate selection (delegated to caller)
- Cycle detection is basic DFS, not Tarjan's SCC
- `Resolver::resolve()` returns `Graph` directly without error for unresolvable (eprintln warning)
- The resolver handles version selection only — recursive transitive resolution is done in `ara-cli`

## Test

`cargo test -p ara-resolver`

## Dependencies

- External: `serde`, `serde_json`, `thiserror`
- Internal: `ara-types`, `ara-util` (for `hash::compute` in `compute_hash`)
