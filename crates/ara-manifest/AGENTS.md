# ara-manifest

Parsing and generation of `package.json` and `ara.toml` manifest files.

## Modules & Public API

**`types.rs`**:
- `Manifest { project, deps, workspace, scripts, security, build, package_json_extras }`
- `DependencyEntry { name, source, kind, version, repo, url, commit, path }`
- `DepEntryRaw`, `Security { risk_threshold, require_review }`, `Build { hermetic, offline_first }`

**`parser.rs`**:
- `parse(content) -> Result<Manifest>` — parse `ara.toml` content (TOML format)
- `ManifestParseError` enum
- Validates source types (`npm`, `registry`, `github`, `git`, `local`, `workspace`), risk levels, constraint formats, and package names (rejects path traversal)

**`package_json.rs`**:
- `parse_package_json(content) -> Result<Manifest>` — parse `package.json` content
- `generate_package_json(manifest) -> String` — pretty-printed JSON output, preserves unknown fields via `package_json_extras`
- Handles `workspace:` protocol prefix on dependency versions

## Conventions

- No `async`, no `unsafe`
- `ara.toml` is optional; `package.json` is the primary manifest
- Unknown/extra fields in `package.json` are preserved via raw JSON string storage for round-trip fidelity
- Name validation blocks empty, null bytes, absolute paths, `..`/`.` traversal

## Test

`cargo test -p ara-manifest`

## Dependencies

- External: `serde`, `serde_json`, `toml`, `thiserror`
- Internal: `ara-types`
