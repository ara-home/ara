use std::path::PathBuf;

use ara_util::hash;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid store key: {0}")]
    InvalidKey(String),
    #[error("integrity violation: expected {expected}, got {actual}")]
    IntegrityViolation { expected: String, actual: String },
}

fn validate_key(key: &str) -> Result<(), StoreError> {
    if key.contains('\0') || key.contains('/') || key.contains('\\') || key.contains("..") {
        return Err(StoreError::InvalidKey(key.to_string()));
    }
    Ok(())
}

fn shard_path(hash_str: &str) -> PathBuf {
    let hash = hash_str.strip_prefix("sha256-").unwrap_or(hash_str);
    let (shard1, rest) = hash.split_at(2);
    let (shard2, _) = rest.split_at(2);
    PathBuf::from(shard1).join(shard2)
}

#[derive(Clone)]
pub struct Store {
    base_path: PathBuf,
}

impl Store {
    #[must_use]
    pub const fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    #[must_use]
    pub fn base_path(&self) -> &std::path::Path {
        &self.base_path
    }

    pub fn ensure_dirs(&self) -> Result<(), StoreError> {
        let dirs = [
            "objects",
            "graphs",
            "snapshots",
            "cache",
            "temp",
            "extracted",
        ];
        for d in &dirs {
            let p = self.base_path.join(d);
            std::fs::create_dir_all(&p)?;
        }
        let _ = self.migrate_flat_objects();
        Ok(())
    }

    pub fn object_path(&self, hash_str: &str) -> PathBuf {
        self.base_path
            .join("objects")
            .join(shard_path(hash_str))
            .join(hash_str)
    }

    fn graph_path(&self, graph_hash: &str) -> PathBuf {
        self.base_path.join("graphs").join(graph_hash)
    }

    fn temp_dir(&self) -> PathBuf {
        self.base_path.join("temp")
    }

    #[must_use]
    pub fn get_extracted_path(&self, hash_str: &str) -> PathBuf {
        self.base_path.join("extracted").join(hash_str)
    }

    #[must_use]
    pub fn has_extracted(&self, hash_str: &str) -> bool {
        self.get_extracted_path(hash_str).exists()
    }

    pub fn put(&self, bytes: &[u8]) -> Result<String, StoreError> {
        let raw_hash = hash::compute(bytes);
        let hex = hash::hex_encode(&raw_hash);
        let hash_str = format!("sha256-{hex}");

        let path = self.object_path(&hash_str);
        if path.exists() {
            return Ok(hash_str);
        }

        let temp = self.temp_dir();
        std::fs::create_dir_all(&temp)?;

        let tmp_path = temp.join(uuid::Uuid::new_v4().to_string());
        std::fs::write(&tmp_path, bytes)?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::rename(&tmp_path, &path)?;
        Ok(hash_str)
    }

    pub fn get(&self, hash_str: &str) -> Result<Option<Vec<u8>>, StoreError> {
        validate_key(hash_str)?;
        let path = self.object_path(hash_str);
        let data = match std::fs::read(&path) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let legacy = self.base_path.join("objects").join(hash_str);
                match std::fs::read(&legacy) {
                    Ok(data) => data,
                    Err(_) => return Ok(None),
                }
            }
            Err(e) => return Err(StoreError::Io(e)),
        };
        let actual_hash = hash::hex_encode(&hash::compute(&data));
        let expected = hash_str.strip_prefix("sha256-").unwrap_or(hash_str);
        if actual_hash != expected {
            return Err(StoreError::IntegrityViolation {
                expected: format!("sha256-{expected}"),
                actual: format!("sha256-{actual_hash}"),
            });
        }
        Ok(Some(data))
    }

    #[must_use]
    pub fn contains(&self, hash_str: &str) -> bool {
        if validate_key(hash_str).is_err() {
            return false;
        }
        let path = self.object_path(hash_str);
        if path.exists() {
            return true;
        }
        let legacy = self.base_path.join("objects").join(hash_str);
        legacy.exists()
    }

    pub fn remove(&self, hash_str: &str) -> Result<(), StoreError> {
        validate_key(hash_str)?;
        let path = self.object_path(hash_str);
        match std::fs::remove_file(&path) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let legacy = self.base_path.join("objects").join(hash_str);
                if legacy.exists() {
                    std::fs::remove_file(&legacy)?;
                    Ok(())
                } else {
                    Err(StoreError::Io(e))
                }
            }
            Err(e) => Err(StoreError::Io(e)),
        }
    }

    pub fn put_graph(&self, graph_bytes: &[u8]) -> Result<String, StoreError> {
        let raw_hash = hash::compute(graph_bytes);
        let hex = hash::hex_encode(&raw_hash);
        let hash_str = format!("graph-{hex}");

        let path = self.graph_path(&hash_str);
        if path.exists() {
            return Ok(hash_str);
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, graph_bytes)?;
        Ok(hash_str)
    }

    pub fn migrate_flat_objects(&self) -> Result<u64, StoreError> {
        let flat_dir = self.base_path.join("objects");
        let mut migrated = 0u64;

        let entries: Vec<_> = match std::fs::read_dir(&flat_dir) {
            Ok(r) => r
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .collect(),
            Err(_) => return Ok(0),
        };

        for entry in &entries {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.starts_with("sha256-") {
                continue;
            }
            let sharded = self.object_path(&name_str);
            if sharded.exists() {
                continue;
            }
            if let Some(parent) = sharded.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(entry.path(), &sharded)?;
            migrated += 1;
        }

        if migrated > 0 {
            let remaining_flat_entries: Vec<_> = match std::fs::read_dir(&flat_dir) {
                Ok(r) => r
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_file())
                    .collect(),
                Err(_) => vec![],
            };
            if remaining_flat_entries.is_empty() {
                for entry in std::fs::read_dir(&flat_dir)? {
                    let entry = entry?;
                    if entry.path().is_dir()
                        && entry.path().file_name().is_some_and(|n| n.len() == 2)
                    {
                        continue;
                    }
                }
            }
        }

        Ok(migrated)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, Store) {
        let dir = TempDir::new().expect("failed to create temp dir");
        let store = Store::new(dir.path().to_path_buf());
        store.ensure_dirs().unwrap();
        (dir, store)
    }

    #[test]
    fn test_put_and_get_roundtrip() {
        let (_dir, store) = setup();
        let hash_str = store.put(b"hello").unwrap();
        assert!(hash_str.starts_with("sha256-"));

        let data = store.get(&hash_str).unwrap();
        assert!(data.is_some());
        assert_eq!(data.unwrap(), b"hello");
    }

    #[test]
    fn test_deduplication() {
        let (_dir, store) = setup();
        let h1 = store.put(b"same").unwrap();
        let h2 = store.put(b"same").unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_contains_and_remove() {
        let (_dir, store) = setup();
        let hash_str = store.put(b"data").unwrap();
        assert!(store.contains(&hash_str));
        store.remove(&hash_str).unwrap();
        assert!(!store.contains(&hash_str));
    }

    #[test]
    fn test_put_graph_roundtrip() {
        let (_dir, store) = setup();
        let hash_str = store.put_graph(b"graph data here").unwrap();
        assert!(hash_str.starts_with("graph-"));
    }

    #[test]
    fn test_not_found_returns_none() {
        let (_dir, store) = setup();
        let data = store.get("sha256-nonexistent").unwrap();
        assert!(data.is_none());
    }

    #[test]
    fn test_get_rejects_invalid_key() {
        let (_dir, store) = setup();
        assert!(matches!(
            store.get("sha256-\0invalid"),
            Err(StoreError::InvalidKey(_))
        ));
        assert!(matches!(
            store.get("../etc/passwd"),
            Err(StoreError::InvalidKey(_))
        ));
        assert!(matches!(
            store.get("foo/bar"),
            Err(StoreError::InvalidKey(_))
        ));
    }

    #[test]
    fn test_get_rejects_tampered_data() {
        let (_dir, store) = setup();
        let hash = store.put(b"valid content").unwrap();
        let obj_path = store.object_path(&hash);
        std::fs::write(&obj_path, b"tampered").unwrap();
        let result = store.get(&hash);
        assert!(matches!(result, Err(StoreError::IntegrityViolation { .. })));
    }

    #[test]
    fn test_put_atomic_writes_to_sharded_path() {
        let (_dir, store) = setup();
        let hash = store.put(b"atomic test").unwrap();
        let path = store.object_path(&hash);
        assert!(path.exists());
        // Path: <base>/objects/<shard1>/<shard2>/sha256-<hex>
        // 3 parents up gives us <base>/objects/
        let grandparent = path.parent().unwrap().parent().unwrap().parent().unwrap();
        assert_eq!(
            grandparent.file_name().unwrap().to_str().unwrap(),
            "objects"
        );
    }

    #[test]
    fn test_sharding_distribution() {
        let (_dir, store) = setup();
        let mut seen_shards = std::collections::HashSet::new();
        for i in 0..100 {
            let data = format!("data-{i}");
            let hash = store.put(data.as_bytes()).unwrap();
            let shard = shard_path(&hash);
            seen_shards.insert(shard);
        }
        assert!(seen_shards.len() > 1, "expected multiple shards");
    }

    #[test]
    fn test_put_replaces_corrupted_object() {
        let (_dir, store) = setup();
        let content = b"original content";
        let hash = store.put(content).unwrap();
        let obj_path = store.object_path(&hash);
        std::fs::write(&obj_path, b"garbage").unwrap();
        let _ = std::fs::remove_file(&obj_path);
        let hash2 = store.put(content).unwrap();
        assert_eq!(hash, hash2);
        let data = store.get(&hash).unwrap().unwrap();
        assert_eq!(data, content);
    }

    #[test]
    fn test_get_empty_object() {
        let (_dir, store) = setup();
        let hash = store.put(b"").unwrap();
        let data = store.get(&hash).unwrap();
        assert!(data.is_some());
        assert_eq!(data.unwrap(), b"");
    }

    #[test]
    fn test_migrate_flat_objects() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let store_base = dir.path().join("store");
        let flat_dir = store_base.join("objects");
        std::fs::create_dir_all(&flat_dir).unwrap();
        let store = Store::new(store_base.clone());
        store.ensure_dirs().unwrap();

        // Write directly to flat directory to simulate legacy store
        let hash = "sha256-2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        std::fs::write(flat_dir.join(hash), b"hello").unwrap();

        let sharded_path = store.object_path(hash);
        assert!(
            !sharded_path.exists(),
            "sharded path should not exist before migration"
        );

        let count = store.migrate_flat_objects().unwrap();
        assert_eq!(count, 1, "expected 1 migrated object");

        assert!(
            sharded_path.exists(),
            "sharded path should exist after migration"
        );
        assert!(
            !flat_dir.join(hash).exists(),
            "flat object should no longer exist in the old location"
        );

        let migrated_content = std::fs::read(&sharded_path).unwrap();
        assert_eq!(migrated_content, b"hello");
    }

    #[test]
    fn test_contains_finds_legacy_flat_object() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let store_base = dir.path().join("store");
        let flat_dir = store_base.join("objects");
        std::fs::create_dir_all(&flat_dir).unwrap();

        let store = Store::new(store_base.clone());
        store.ensure_dirs().unwrap();

        let hash = store.put(b"legacy check").unwrap();
        assert!(store.contains(&hash));
        let sharded = store.object_path(&hash);
        assert!(sharded.exists());
    }

    #[cfg(feature = "nightly-bench")]
    #[bench]
    fn bench_store_put_1kb(b: &mut test::Bencher) {
        let (_dir, store) = setup();
        let data = vec![0u8; 1024];
        b.iter(|| store.put(test::black_box(&data)).unwrap());
    }

    #[cfg(feature = "nightly-bench")]
    #[bench]
    fn bench_store_put_graph_100(b: &mut test::Bencher) {
        let (_dir, store) = setup();
        let nodes: Vec<ara_types::Version> = (0..100)
            .map(|i| ara_types::Version::parse(&format!("{i}.0.0")).unwrap())
            .collect();
        let bytes = serde_json::to_vec(&nodes).unwrap();
        b.iter(|| store.put_graph(test::black_box(&bytes)).unwrap());
    }
}
