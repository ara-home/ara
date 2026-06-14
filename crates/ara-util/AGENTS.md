# ara-util

Utility crate with SHA-256 hashing and a shared HTTP client. Zero internal ara dependencies.

## Modules & Public API

**`hash`**:
- `compute(bytes) -> [u8; 32]` — SHA-256
- `compute_sha512(bytes) -> [u8; 64]` — SHA-512
- `hex_encode(hash)`, `hex_encode_64(hash)` — hex formatting
- `verify_integrity(content, integrity) -> bool` — SRI-style verification (sha256 hex, sha512 base64)
- `format_sha256(content) -> String` — output as `sha256-<hex>`

**`http`**:
- `HttpError` — Request, StatusNotOk, MaxRetries, InsecureUrl
- `HttpClient` — shared connection pool via `OnceLock`
  - `HttpClient::new() -> Self`
  - `get(url) -> Result<Vec<u8>>` — GET with 3 retries, exponential backoff, 120s timeout
- Plain HTTP rejected unless `ARA_ALLOW_HTTP=1` env var set (localhost exempt)

## Conventions

- `#[must_use]` on hash functions and public accessors
- HTTP/2 tuned for npm registry patterns (large windows, 512 idle connections)
- Retry only on server errors (5xx) or connection failures

## Test

`cargo test -p ara-util`

## Dependencies

- External: `sha2`, `hex`, `base64`, `reqwest`, `tokio`, `thiserror`
- Dev: `mockito`
- Internal: none
