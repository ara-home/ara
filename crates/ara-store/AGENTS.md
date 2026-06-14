# ara-store

Content-addressable storage with sharded filesystem layout and SQLite index. Depends on `ara-util` for hashing.

## Modules & Public API

**`cas`** (content storage):
- `Store { base_path }` — `Store::new(path)`, `ensure_dirs()`
- `put(bytes) -> Result<String>` — store bytes, return `sha256-<hex>` (atomic write via temp + rename)
- `get(hash_str) -> Result<Option<Vec<u8>>>` — retrieve with integrity verification
- `contains(hash_str) -> bool` — checks sharded + legacy flat layout
- `remove(hash_str)` — delete object
- `put_graph(bytes)` — store graph snapshot
- `migrate_flat_objects()` — migrate legacy flat layout to sharded
- `object_path(hash_str)`, `get_extracted_path(hash_str)`, `has_extracted(hash_str)`

**`index`** (SQLite metadata):
- `StoreIndex` — `StoreIndex::new(db_path)`, auto-migrates from legacy JSON
- `lookup(cache_key)`, `insert(cache_key, hash, ...)`, `remove(cache_key)`
- `get_active_hashes()`, `get_orphan_hashes()`, `get_all_hashes()`
- `batch_insert(entries)` — single-transaction bulk insert
- `clean_orphan_entries()` — delete refcount ≤ 0 entries

## Conventions

- No `async` (synchronous I/O; callers use `tokio::task::spawn_blocking`)
- No `unsafe`
- Sharded layout: `objects/<2-char>/<2-char>/sha256-<hex>`
- Legacy flat layout (`objects/sha256-<hex>`) supported for reads + migration
- Key validation rejects null bytes, `/`, `\\`, `..` (path traversal protection)
- SQLite WAL mode, busy timeout 5000ms, `Mutex<Connection>` for thread safety

## Test

`cargo test -p ara-store`

## Dependencies

- External: `rusqlite`, `serde_json`, `uuid`, `thiserror`
- Dev: `tempfile`
- Internal: `ara-util`
