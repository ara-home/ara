# ara-lockfile

Lockfile generation and parsing for `ara.lock`. Zero internal ara dependencies.

## Modules & Public API

**`types.rs`**:
- `Lockfile { version, graph, packages }` — serde Serialize/Deserialize
- `GraphMeta { resolver, generated_at, graph_hash }`
- `PackageEntry { name, version, source, package_hash, integrity, ... }`
- `SecurityMeta`, `SbomMeta`

**`generator.rs`**:
- `generate(lockfile) -> String` — manual TOML serialization (no serde) for precise formatting control

**`parser.rs`**:
- `parse(content) -> Lockfile` — deserializes via `toml::from_str` with serde
- Validates: version must be 1, resolver must be `"mvs"`, package_hash must match `sha256-<64hex>`
- `LockfileParseError` enum

## Conventions

- No `async`, no `unsafe`
- Output serialization is **manual** (string building); input deserialization uses **serde**
- Only lockfile version 1 supported
- Hash format: `sha256-` prefix followed by 64 lowercase hex chars

## Test

`cargo test -p ara-lockfile`

## Dependencies

- External: `serde`, `toml`, `thiserror`
- Internal: none
