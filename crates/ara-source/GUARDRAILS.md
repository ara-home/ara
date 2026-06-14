# GUARDRAILS.md — ara-source

---

## SIGN #1: Enum Dispatch
**Trigger:** Replacing the `Source` enum with `#[async_trait]`, trait objects, or dynamic dispatch
**Instruction:** Keep the `Source` enum pattern. If adding a new source type, add a new variant and implement `resolve()` / `fetch()` in the match arms.
**Reason:** Enum dispatch avoids trait object overhead, keeps async bounds simple, and makes pattern matching exhaustive at compile time. Changing to async traits would be a major refactoring across all callers.
**Provenance:** Architectural decision documented in root `AGENTS.md`.

---

## SIGN #2: URL Validation Bypass
**Trigger:** Removing URL validation in `git.rs` (scheme blocking), `tarball.rs` (HTTPS enforcement), or `url.rs` (install spec parsing)
**Instruction:** Preserve all security URL validation. `file://` and `ext:` schemes must remain blocked in `GitSource`. `validate_tarball_url()` must enforce HTTPS for remote URLs.
**Reason:** These validations prevent supply-chain attacks via malicious install specs. Removing them would allow fetching packages from insecure or local sources.
**Provenance:** Security invariants enforced since initial source implementations.

---

## SIGN #3: Registry Cache Format
**Trigger:** Modifying the registry metadata cache format, TTL, or integrity verification
**Instruction:** Keep backward compatibility with the existing cache format. If changing the format, implement a migration path. The SHA-256 integrity sidecar files must remain.
**Reason:** Users have cached metadata that would need to be re-fetched. The 7-day TTL is tuned for npm registry update frequency.
**Provenance:** Disk caching in `registry.rs` with integrity sidecars and TTL of 604800s.
