# GUARDRAILS.md — ara-lockfile

---

## SIGN #1: Serialization Approach
**Trigger:** Changing the generator from manual TOML serialization to `toml::to_string()` with serde, or changing the parser from serde deserialization to manual parsing
**Instruction:** Keep the current approach: manual generation for output (control over formatting), serde deserialization for input. If changing, justify with a clear reason.
**Reason:** Manual serialization was chosen intentionally to control formatting precisely. Serde deserialization is used for input parsing because it's simpler and the input format is well-defined.
**Provenance:** Architectural decision in `generator.rs` and `parser.rs`.

---

## SIGN #2: Hash Format Validation
**Trigger:** Changing the `package_hash` validation pattern (`sha256-<64hex>`)
**Instruction:** Keep the `sha256-<64hex>` format for `package_hash`. If adding new hash formats, validate them separately and document the format version.
**Reason:** The hash format is the integrity anchor of the lockfile. Changing it without migration breaks every existing `ara.lock` file.
**Provenance:** Validation in `parser.rs` since initial lockfile implementation.

---

## SIGN #3: Version Constraint
**Trigger:** Adding support for lockfile version 2 or changing the resolver from `"mvs"` without migration
**Instruction:** If adding a new version, implement a forward-compatible reader that can parse both old and new formats. The resolver must remain `"mvs"` unless a new resolver algorithm is added alongside it.
**Reason:** The lockfile version and resolver field are critical for reproducibility. Old lockfiles must remain readable.
**Provenance:** Validation: version must be 1, resolver must be `"mvs"`.
