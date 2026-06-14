# ara-source

Package fetching from multiple backends: npm registry, GitHub, git, tarball URLs, local paths, and workspace members.

## Modules & Public API

**`lib.rs`** — enum dispatch (no async traits):
- `Source { Local, Workspace, Git, Github, Registry, Url }`
- `Source::resolve(&self, name) -> Result<String>`
- `Source::fetch(&self, identity) -> Result<Vec<u8>>`
- `SourceError` enum

**`registry.rs`** (npm registry client):
- `RegistrySource::new(url)`, `warmup()` — pre-warm HTTP/2
- `fetch_metadata(name)`, `resolve(name)`, `resolve_matching(name, constraint)`
- `fetch(identity)` — download tarball with integrity verification
- `resolve_and_get_deps(name, constraint)`, `get_deps_for_version(name, version)`

**`git.rs`**: `GitSource::new(url, commit)` — shells out to `git` + `tar` CLI. Rejects `file://` and `ext:` schemes.

**`github.rs`**: `GithubSource::new(repo)` — fetches from `api.github.com/repos/{repo}/tarball/{ref}`

**`local.rs`**: `LocalSource::new(path)` — creates gzipped tarball from local directory
**`workspace.rs`**: `pub use super::local::LocalSource as WorkspaceSource`

**`tarball.rs`**: `TarballSource::new(url)` — HTTPS-only URL fetch. Also: `identity_from_tarball()`, `name_from_url()`

**`url.rs`**: `parse_install_spec(spec) -> InstallTarget` — parses CLI install specs (npm, GitHub shorthand, Git, tarball). Handles scoped packages, SCP-style URLs, fragments.

## Conventions

- Async (tokio), no `unsafe`
- Registry metadata cached on disk at `~/.ara/cache/metadata/` with 7-day TTL and integrity sidecars
- Cache format supports legacy npm metadata (`{"body": {...}}`)
- `WorkspaceSource` is a type alias for `LocalSource`

## Test

`cargo test -p ara-source`

## Dependencies

- External: `flate2`, `reqwest`, `serde_json`, `tar`, `tempfile`, `thiserror`
- Dev: `mockito`, `tokio`
- Internal: `ara-types`, `ara-util`
