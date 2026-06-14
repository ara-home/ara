# GUARDRAILS.md — ara-util

---

## SIGN #1: Hash Algorithm Change
**Trigger:** Modifying `hash::compute()` to use a non-SHA-256 algorithm, or changing the output format
**Instruction:** Keep SHA-256 as the default hash. Any change requires updating `ara-store`, `ara-lockfile`, `ara-resolver`, and `ara-cli` in lockstep. Verify all hash consumers still work.
**Reason:** SHA-256 is baked into the store sharding, lockfile format, graph hashing, and integrity verification. Changing it without coordinated updates breaks the entire storage layer.
**Provenance:** Chosen at project inception. Referenced by 4+ crates.

---

## SIGN #2: HTTP Client Security Bypass
**Trigger:** Removing or weakening HTTPS enforcement, TLS verification, or the `ARA_ALLOW_HTTP` env-var pattern
**Instruction:** Keep HTTPS-only as default. The `ARA_ALLOW_HTTP=1` escape hatch must remain opt-in and documented. Do NOT make HTTP the default or remove the env-var check.
**Reason:** Package fetches transmit code that gets executed. HTTP downgrade attacks could inject malicious packages. The env-var pattern allows local development without compromising defaults.
**Provenance:** Security invariant. HTTP→HTTPS upgrade enforced at `HttpClient` level.

---

## SIGN #3: Retry Logic Removal
**Trigger:** Removing or reducing retry attempts, backoff delays, or error-type filtering in `HttpClient::get()`
**Instruction:** Keep at least 3 retries with exponential backoff. Only retry on retryable errors (server 5xx, connection failures). Do NOT retry on 4xx client errors.
**Reason:** Network failures are expected in real-world registry interactions. The retry logic was added after flaky CI builds caused by transient npm registry errors.
**Provenance:** Added during CI stabilization.
