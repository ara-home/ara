# ara-cli

Binary entrypoint and command dispatch. Depends on **all** other ara crates. Largest crate in the workspace.

## Structure

```
main.rs         # #[tokio::main] entry, error display with {:#}
lib.rs          # pub mod cli
version.rs      # VERSION from env!("ARA_VERSION") (build-time injetado)
cli/
├── mod.rs      # Cli (clap derive) + Commands enum + dispatch
├── install.rs  # ~2850 lines — core install pipeline
├── analyze.rs  # cmd_analyze / cmd_audit
├── run.rs      # cmd_run with sandbox profile
├── x.rs        # cmd_x (like npx)
├── prompt.rs   # Interactive security prompt with colored output
└── gc.rs       # cmd_gc (dry-run, aggressive, normal)
```

## Commands

| Command | Key Flags |
|---|---|
| `install [deps...]` | `--save-dev`, `--force`, `--offline`, `--non-interactive`, `--package-lock` |
| `add <deps...>` | Same as install (alias) |
| `x <pkg> [args]` | Trailing args passed to binary |
| `run <script>` | `--profile` (open, restricted, hermetic) |
| `analyze [path]` | Security analysis |
| `audit [path]` | Extended analysis with summary |
| `gc` | `--dry-run`, `--aggressive` |
| `build` | Not yet implemented |
| `publish` | Not yet implemented |
| `trust` | Not yet implemented |

## Conventions

- All handlers return `anyhow::Result<()>` with `.context()` chaining
- `pub mod install` (needed by `x.rs`); other cli modules are private `mod`
- `pub(crate)` on handler functions like `cmd_install`, `cmd_x`, etc.
- `#[allow(clippy::multiple_crate_versions)]` on main.rs
- `#[cfg(feature = "nightly-bench")]` for benchmarks
- Version injected at build time via `ARA_VERSION` env var (cargo-dist/CI)
- Progress bars via `indicatif`

## Test

Unit: `cargo test -p ara-cli`
Integration (fixtures): `cargo test -p ara-cli --test fixture_test`
Benchmarks: `cargo bench -p ara-cli`

## Dependencies

- External: `clap`, `tokio`, `anyhow`, `thiserror`, `indicatif`, `base64`, `chrono`, `flate2`, `futures`, `glob`, `hex`, `serde`, `serde_json`, `tar`, `walkdir`
- Dev: `mockito`, `tempfile`, `codspeed-criterion-compat`
- Internal: `ara-types`, `ara-util`, `ara-store`, `ara-source`, `ara-analysis`, `ara-manifest`, `ara-lockfile`, `ara-resolver`, `ara-sandbox` (everything)
