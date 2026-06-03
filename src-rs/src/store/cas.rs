use std::path::{Path, PathBuf};

use crate::util::hash;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("path contains null byte")]
    NullByte,
}

pub struct Store {
    base_path: PathBuf,
}

impl Store {
    #[must_use]
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    pub fn ensure_dirs(&self) -> Result<(), StoreError> {
        let dirs = ["objects", "graphs", "snapshots", "cache", "temp"];
        for d in &dirs {
            let p = self.base_path.join(d);
            std::fs::create_dir_all(&p)?;
        }
        Ok(())
    }

    fn object_path(&self, hash_str: &str) -> PathBuf {
        self.base_path.join("objects").join(hash_str)
    }

    fn graph_path(&self, graph_hash: &str) -> PathBuf {
        self.base_path.join("graphs").join(graph_hash)
    }

    pub fn put(&self, bytes: &[u8]) -> Result<String, StoreError> {
        let raw_hash = hash::compute(bytes);
        let hex = hash::hex_encode(&raw_hash);
        let hash_str = format!("sha256-{hex}");

        let path = self.object_path(&hash_str);
        if path.exists() {
            return Ok(hash_str);
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
        Ok(hash_str)
    }

    pub fn get(&self, hash_str: &str) -> Result<Option<Vec<u8>>, StoreError> {
        if hash_str.contains('\0') {
            return Err(StoreError::NullByte);
        }
        let path = self.object_path(hash_str);
        match std::fs::read(&path) {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StoreError::Io(e)),
        }
    }

    #[must_use]
    pub fn contains(&self, hash_str: &str) -> bool {
        if hash_str.contains('\0') {
            return false;
        }
        self.object_path(hash_str).exists()
    }

    pub fn remove(&self, hash_str: &str) -> Result<(), StoreError> {
        if hash_str.contains('\0') {
            return Err(StoreError::NullByte);
        }
        let path = self.object_path(hash_str);
        std::fs::remove_file(&path)?;
        Ok(())
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
}

#[cfg(test)]
mod tests {
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
}
