use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;

use crate::store::cas::Store;

pub(crate) fn cmd_gc() -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let store_base = PathBuf::from(&home).join(".ara").join("store");
    cmd_gc_in(&store_base)
}

pub(crate) fn cmd_gc_in(store_base: &std::path::Path) -> Result<()> {
    let store = Store::new(store_base.to_path_buf());

    let index_path = store_base.join("index.json");

    let active_hashes: std::collections::HashSet<String> = if index_path.exists() {
        let content = std::fs::read_to_string(&index_path)?;
        let map: HashMap<String, String> = serde_json::from_str(&content).unwrap_or_default();
        map.into_values().collect()
    } else {
        println!("No store index found. Nothing to clean.");
        return Ok(());
    };

    let objects_dir = store_base.join("objects");
    let mut removed = 0u64;
    let mut total_size = 0u64;

    if objects_dir.exists() {
        for entry in std::fs::read_dir(&objects_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !active_hashes.contains(name) {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        total_size += meta.len();
                    }
                    store.remove(name)?;
                    removed += 1;
                }
            }
        }
    }

    let graphs_dir = store_base.join("graphs");
    if graphs_dir.exists() {
        for entry in std::fs::read_dir(&graphs_dir)? {
            let entry = entry?;
            if entry.path().is_file() {
                std::fs::remove_file(entry.path())?;
                removed += 1;
            }
        }
    }

    if removed > 0 {
        println!("Removed {removed} orphaned objects ({total_size} bytes freed)");
    } else {
        println!("Store is clean. No orphaned objects found.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_cmd_gc_clean_store() {
        let store_base = tempfile::tempdir().unwrap();
        let objects = store_base.path().join("objects");
        let graphs = store_base.path().join("graphs");
        std::fs::create_dir_all(&objects).unwrap();
        std::fs::create_dir_all(&graphs).unwrap();

        let mut index = std::collections::HashMap::new();
        index.insert("test-pkg@1.0.0".to_string(), "sha256-active".to_string());
        std::fs::write(
            store_base.path().join("index.json"),
            serde_json::to_string(&index).unwrap(),
        )
        .unwrap();

        std::fs::write(objects.join("sha256-active"), b"content").unwrap();
        std::fs::write(objects.join("sha256-orphan"), b"orphan").unwrap();

        cmd_gc_in(store_base.path()).unwrap();

        assert!(objects.join("sha256-active").exists());
        assert!(!objects.join("sha256-orphan").exists());
    }

    #[test]
    fn test_cmd_gc_no_index() {
        let store_base = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(store_base.path().join("objects")).unwrap();
        std::fs::write(store_base.path().join("objects").join("some-hash"), b"data").unwrap();
        cmd_gc_in(store_base.path()).unwrap();
    }
}
