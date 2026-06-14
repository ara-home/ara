# GUARDRAILS.md — ara-analysis

---

## SIGN #1: Anyhow Usage
**Trigger:** Replacing `anyhow::Result` with a custom `thiserror` type for public API functions (`analyze_package()`, `scan_package()`)
**Instruction:** Keep `anyhow::Result` for `analyze_package()` and `scan_package()`. This is the only library crate where `anyhow` is the established pattern.
**Reason:** `ara-analysis` is intentionally the one library crate that uses `anyhow` for its public API. Changing to `thiserror` would require wrapping every lower-level error and would not provide meaningful error-type discrimination at call sites.
**Provenance:** Established pattern — diverging from other lib crates intentionally.

---

## SIGN #2: Deduplication Key
**Trigger:** Changing the deduplication logic in `analyzer.rs` (the `HashSet<(file_idx, pattern_id, byte_offset)>` key)
**Instruction:** Preserve the three-key deduplication (file + pattern + byte offset). If changing the key, verify that all duplicates are still correctly filtered and no legitimate findings are lost.
**Reason:** The dedup key ensures each finding is reported once per line. Changing the key could cause duplicate flood or missed findings.
**Provenance:** Dedup logic implemented since initial analysis engine.

---

## SIGN #3: File Scan Limits
**Trigger:** Increasing the 1 MB file size limit, removing skipped directories (e.g., `node_modules/`, `.git/`, `dist/`), or enabling symlink traversal
**Instruction:** Keep current limits unchanged. If increases are needed, justify the security impact. Never enable `walkdir::WalkDir::follow_links(true)`.
**Reason:** Large files are skipped to prevent DoS via archive bombs. Ignored directories prevent scanning non-package code. Symlink traversal could escape the package directory.
**Provenance:** Security invariant. Limits defined in `scanner.rs` since initial implementation.
