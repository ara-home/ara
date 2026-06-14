# GUARDRAILS.md — ara-manifest

---

## SIGN #1: Round-Trip Fidelity
**Trigger:** Modifying `generate_package_json()` or `parse_package_json()` in a way that could lose unknown fields from `package_json_extras`
**Instruction:** Always preserve unknown fields. Use `#[serde(flatten)]` or raw JSON storage. Test round-trip fidelity: parse → generate → parse and compare.
**Reason:** Other tools (npm, yarn, renovate) may add fields to `package.json`. Losing them on `ara add` would corrupt the user's project configuration.
**Provenance:** `package_json_extras` field and `#[serde(flatten)]` pattern since initial manifest implementation.

---

## SIGN #2: Name Validation Bypass
**Trigger:** Removing or weakening package name validation in `parser.rs` (`validate_name()`)
**Instruction:** Keep validation for: empty names, null bytes, absolute paths, `..`/`.` traversal, and overly long names. Expand validation if new attack vectors are discovered.
**Reason:** Names are used in filesystem paths and URLs. Invalid names could cause path traversal or injection attacks.
**Provenance:** Security invariant. `validate_name()` in `parser.rs` since initial implementation.

---

## SIGN #3: Workspace Protocol Detection
**Trigger:** Modifying the `workspace:` prefix detection in dependency version parsing
**Instruction:** Preserve the `workspace:` prefix detection. If changing the format, ensure both old and new formats are supported during a transition period.
**Reason:** The `workspace:` protocol is a user-facing feature. Breaking it silently would corrupt monorepo dependency declarations.
**Provenance:** Workspace protocol documented in `README.md` and implemented in `package_json.rs`.
