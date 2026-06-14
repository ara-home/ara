---
name: fixture-test
description: Create a fixture-based integration test for ara-cli using mockito HTTP mocks. Covers directory structure, mock setup, and assertions.
license: MIT
compatibility: opencode
---

## Steps

1. **Create fixture directory**: `crates/ara-cli/tests/fixtures/<category>/<test-name>/`
   - `<category>` groups tests (e.g. `valid`, `edge`, `malformed`, `security`, `workspace`)
   - Add `package.json` with the dependencies and metadata for the test
   - Optionally add `ara.toml` for advanced config
2. **Create mock packages** (if the test needs registry packages):
   - Add tarballs or use `make_minimal_tarball()` / `make_tarball_with_files()` helpers
   - Register them with `mock_npm_package()` for mockito endpoints
3. **Register the test** in `crates/ara-cli/tests/fixture_test.rs`:
   - Start a `mockito` server
   - Register mock endpoints for registry/tarball URLs
   - Run the `ara` binary as a subprocess using the fixture directory as cwd
   - Assert exit code with `assert_eq!(output.status.code(), Some(0))`
   - Assert lockfile exists: `assert!(lockfile_path.exists())`
4. **Run**: `cargo test -p ara-cli --test fixture_test -- <test-name-pattern>`
5. **Verify** the full suite still passes: `cargo test -p ara-cli --test fixture_test`

## When to use

Use when asked to add a new integration test for ara-cli, or when testing a specific install/analysis/run scenario that requires end-to-end coverage.
