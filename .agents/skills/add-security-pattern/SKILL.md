---
name: add-security-pattern
description: Add a new security detection pattern to ara-analysis. Covers patterns.rs entry, tests, and README documentation.
license: MIT
compatibility: opencode
---

## Steps

1. **Add the `Pattern` entry** in `all_patterns()` in `crates/ara-analysis/src/patterns.rs`:
   - Choose a unique kebab-case `id` (e.g. `dangerous-api-call`)
   - Set appropriate `severity` (`Critical`, `High`, `Medium`, `Low`)
   - Write the `regex` — must be a valid Rust regex
   - Set `file_glob` for which file extensions to scan (e.g. `*.{js,ts,jsx,tsx}`)
   - Write a clear `description`
   - Follow the existing `Pattern::new(...)` call style
2. **Add match/no-match tests** in the `#[cfg(test)]` section of `patterns.rs`:
   - Add a test function like `test_<id>()`
   - Call `assert_matches(regex, "matching code")`
   - Call `assert_not_matches(regex, "clean code")`
   - Follow the existing test pattern (see `test_eval_usage()`, `test_credential_access()`, etc.)
3. **If the pattern needs special scanner behavior**, update `scanner.rs` (rare — most patterns only need the regex)
4. **Update the pattern table in `README.md`** at the project root (add row with ID, severity, description)
5. **Verify**: `cargo test -p ara-analysis` then `cargo clippy -p ara-analysis -- -D warnings`

## When to use

Use when asked to add a new security analysis pattern, or when a new type of supply-chain attack vector should be detected by Ara's scanner.
