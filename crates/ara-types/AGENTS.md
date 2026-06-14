# ara-types

Domain types for the Ara package manager. Root of the dependency DAG — zero internal ara dependencies.

## Public API

- `SourceType` — enum: Workspace, Local, Git, Github, Registry, Npm, Url
- `Constraint` — enum: Exact, Caret, Tilde, GreaterOrEqual, Wildcard, And, etc.
- `RiskLevel` — enum: Low, Medium, High, Critical (Ord for severity ordering)
- `PackageIdentity { source, name, version, content_hash, requested_ref }`
- `Finding { pattern, severity, location, description }`
- `AnalysisResult { risk_level, findings }`
- `UnknownSourceType`, `ConstraintParseError` — thiserror types

Key methods: `Constraint::parse()`, `Constraint::satisfied_by()`, `Constraint::parse_version()`

Re-exports `semver::Version` as `pub use semver::Version`.

## Conventions

- No `async`, no `unsafe`, no `anyhow` — pure domain types
- `#[must_use]` on `satisfied_by()`
- Caret (^) is zero-aware: `^0.1.2` pins minor, allows patch
- `Constraint::And` handles compound ranges like `>=1.0.0 <2.0.0`

## Test

`cargo test -p ara-types`

## Dependencies

- External: `serde`, `semver`, `thiserror`
- Internal: none
