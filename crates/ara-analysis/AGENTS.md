# ara-analysis

Security scanner that inspects package source files against 17 regex patterns. The only library crate (besides `ara-cli`) that uses `anyhow`.

## Modules & Public API

**`analyzer.rs`**:
- `analyze_package(path) -> Result<AnalysisResult>` — main entry: scan + match + aggregate risk level
- Returns `AnalysisResult { risk_level, findings }` with deduplicated findings

**`patterns.rs`**:
- `Pattern { id, severity, regex, file_glob, description }`
- `all_patterns() -> &[Pattern]` — 17 const fn patterns (Critical: eval-usage, new-function; High: child-process-exec, prototype-pollution, credential-access; Medium: obfuscated-code, dynamic-require, weak-crypto; etc.)

**`scanner.rs`**:
- `ScannedFile { path, content }`
- `scan_package(path) -> Result<Vec<ScannedFile>>` — walks directory, collects JS/TS files

## Conventions

- No `async`, no `unsafe`
- Uses `regex::RegexSet` for pre-filtering, then per-pattern `find_iter` on matched patterns
- Skips: `node_modules/`, `.git/`, `dist/`, `target/`, `.next/`, `build/`; files > 1 MB; binary files (null bytes); declaration files (`.d.ts`, `.d.mts`, `.d.cts`)
- `walkdir::WalkDir::follow_links(false)` — no symlink traversal
- 17 patterns tested individually with match/no-match examples
- Distinction from other crates: this is the **only library crate using `anyhow`** — keep it that way unless justified

## Test

`cargo test -p ara-analysis`

## Dependencies

- External: `regex`, `walkdir`, `anyhow`, `thiserror`
- Dev: `tempfile`
- Internal: `ara-types`
