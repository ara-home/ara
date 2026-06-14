# GUARDRAILS.md — ara-store

---

## SIGN #1: Sharded Layout Migration
**Trigger:** Modifying the sharded directory layout (`objects/<2-char>/<2-char>/sha256-<hex>`) or removing legacy flat-object support
**Instruction:** Keep backward compatibility with the flat layout for reads. If changing the layout, implement a migration from both layouts AND update `Store::contains()` and `Store::remove()` to check both paths. Never remove legacy support without a migration release note.
**Reason:** Users may have existing stores with flat layout objects. Removing support without migration orphans their cached packages.
**Provenance:** `migrate_flat_objects()` exists specifically for this transition.

---

## SIGN #2: Integrity Verification Bypass
**Trigger:** Removing or disabling integrity re-verification in `Store::get()`
**Instruction:** Always recompute SHA-256 on read and compare with the stored hash. If adding a fast-path that skips verification, make it opt-in and document the security tradeoff.
**Reason:** `Store::get()` re-verifies integrity to detect bit rot, storage corruption, or accidental overwrites. Removing this turns the store into a blind passthrough.
**Provenance:** Design invariant since initial CAS implementation.

---

## SIGN #3: Key Validation Bypass
**Trigger:** Changing `Store::put()` or `Store::get()` key validation to allow null bytes, `/`, `\\`, or `..` in hash strings
**Instruction:** Keep key validation strict. Reject any key containing null bytes, path separators, or parent-directory references.
**Reason:** Keys are used directly in filesystem paths. Path traversal in a hash string could allow writing or reading outside the store directory.
**Provenance:** Security invariant. Validation in `cas.rs` since initial implementation.
