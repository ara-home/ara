use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::Result;

use crate::store::cas::Store;
use crate::store::index::StoreIndex;

pub(crate) fn cmd_gc() -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let store_base = PathBuf::from(&home).join(".ara").join("store");
    cmd_gc_in(&store_base, false, false)
}

pub(crate) fn cmd_gc_dry_run() -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let store_base = PathBuf::from(&home).join(".ara").join("store");
    cmd_gc_in(&store_base, true, false)
}

pub(crate) fn cmd_gc_aggressive() -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let store_base = PathBuf::from(&home).join(".ara").join("store");
    cmd_gc_in(&store_base, false, true)
}

pub(crate) fn cmd_gc_in(
    store_base: &std::path::Path,
    dry_run: bool,
    aggressive: bool,
) -> Result<()> {
    let store = Store::new(store_base.to_path_buf());
    let index_path = store_base.join("index.db");

    let index = if index_path.exists() {
        Some(StoreIndex::new(index_path)?)
    } else {
        None
    };

    let mut total_removed = 0u64;
    let mut total_freed = 0u64;

    if let Some(ref index) = index {
        let (removed, freed) = clean_objects(&store, index, dry_run, aggressive)?;
        total_removed += removed;
        total_freed += freed;
    } else {
        let (removed, freed) = clean_objects_fallback(&store, dry_run)?;
        total_removed += removed;
        total_freed += freed;
    }

    if let Some(ref index) = index {
        let (removed, freed) = clean_extracted(store_base, index, dry_run, aggressive)?;
        total_removed += removed;
        total_freed += freed;
    }

    let temp_removed = clean_temp_dir(store_base, dry_run);
    if temp_removed > 0 {
        let action = if dry_run { "Would remove" } else { "Removed" };
        println!("  {action} {temp_removed} stale temp files");
        total_removed += temp_removed;
    }

    let (graph_removed, graph_freed) = clean_graphs(store_base, dry_run);
    if graph_removed > 0 {
        let action = if dry_run { "Would remove" } else { "Removed" };
        println!("  {action} {graph_removed} graph snapshots ({graph_freed} bytes)");
        total_removed += graph_removed;
        total_freed += graph_freed;
    }

    if let Some(ref index) = index {
        let _ = index.clean_orphan_entries();
    }

    if total_removed == 0 {
        println!("Store is clean. No orphaned objects found.");
    } else if dry_run {
        println!("Dry run complete. Would remove {total_removed} items ({total_freed} bytes)");
    } else {
        println!("Done. Removed {total_removed} items ({total_freed} bytes freed)");
    }

    Ok(())
}

fn clean_objects(
    store: &Store,
    index: &StoreIndex,
    dry_run: bool,
    aggressive: bool,
) -> Result<(u64, u64)> {
    let objects_dir = store.base_path().join("objects");
    if !objects_dir.exists() {
        return Ok((0, 0));
    }

    let on_disk = collect_on_disk_hashes(&objects_dir);

    let target_hashes: std::collections::HashSet<String> = if aggressive {
        let active = index.get_active_hashes()?;
        on_disk.difference(&active).cloned().collect()
    } else {
        let orphan_from_index = index.get_orphan_hashes()?;
        let all_indexed = index.get_all_hashes()?;
        let not_indexed = on_disk
            .difference(&all_indexed)
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        orphan_from_index.union(&not_indexed).cloned().collect()
    };

    if target_hashes.is_empty() {
        return Ok((0, 0));
    }

    let mut removed = 0u64;
    let mut freed = 0u64;

    for hash in &target_hashes {
        let path = store.object_path(hash);
        if let Ok(meta) = std::fs::metadata(&path) {
            freed += meta.len();
        }
        if !dry_run {
            let _ = store.remove(hash);
        }
        removed += 1;
    }

    let action = if dry_run { "Would remove" } else { "Removed" };
    println!("  {action} {removed} orphaned objects ({freed} bytes)");

    Ok((removed, freed))
}

fn clean_objects_fallback(store: &Store, dry_run: bool) -> Result<(u64, u64)> {
    let objects_dir = store.base_path().join("objects");
    if !objects_dir.exists() {
        return Ok((0, 0));
    }

    let mut removed = 0u64;
    let mut freed = 0u64;

    for entry in walkdir::WalkDir::new(&objects_dir)
        .min_depth(3)
        .max_depth(3)
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("sha256-") {
            continue;
        }

        if let Ok(meta) = std::fs::metadata(entry.path()) {
            freed += meta.len();
        }
        if !dry_run {
            let _ = std::fs::remove_file(entry.path());
        }
        removed += 1;
    }

    if removed > 0 {
        let action = if dry_run { "Would remove" } else { "Removed" };
        println!("  {action} {removed} objects (fallback, no index) ({freed} bytes)");
    }

    Ok((removed, freed))
}

fn clean_extracted(
    store_base: &std::path::Path,
    index: &StoreIndex,
    dry_run: bool,
    aggressive: bool,
) -> Result<(u64, u64)> {
    let extracted_dir = store_base.join("extracted");
    if !extracted_dir.exists() {
        return Ok((0, 0));
    }

    let on_disk: HashSet<String> = std::fs::read_dir(&extracted_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("sha256-") {
                Some(name)
            } else {
                None
            }
        })
        .collect();

    let orphan_hashes: HashSet<String> = if aggressive {
        let active = index.get_active_extracted_hashes()?;
        on_disk.difference(&active).cloned().collect()
    } else {
        let orphan_from_index = index.get_orphan_extracted_hashes()?;
        let all_indexed = index.get_all_hashes()?;
        let not_indexed = on_disk
            .difference(&all_indexed)
            .cloned()
            .collect::<HashSet<_>>();
        orphan_from_index.union(&not_indexed).cloned().collect()
    };

    if orphan_hashes.is_empty() {
        return Ok((0, 0));
    }

    let mut removed = 0u64;
    let mut freed = 0u64;

    for hash in &orphan_hashes {
        let path = extracted_dir.join(hash);
        if let Ok(meta) = std::fs::metadata(&path) {
            freed += meta.len();
        }
        if !dry_run {
            let _ = std::fs::remove_dir_all(&path);
        }
        removed += 1;
    }

    let action = if dry_run { "Would remove" } else { "Removed" };
    println!("  {action} {removed} extracted directories ({freed} bytes)");

    Ok((removed, freed))
}

fn clean_graphs(store_base: &std::path::Path, dry_run: bool) -> (u64, u64) {
    let graphs_dir = store_base.join("graphs");
    if !graphs_dir.exists() {
        return (0, 0);
    }

    let mut removed = 0u64;
    let mut freed = 0u64;

    if let Ok(entries) = std::fs::read_dir(&graphs_dir) {
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if entry.path().is_file() {
                if let Ok(meta) = std::fs::metadata(entry.path()) {
                    freed += meta.len();
                }
                if !dry_run {
                    let _ = std::fs::remove_file(entry.path());
                }
                removed += 1;
            }
        }
    }

    (removed, freed)
}

fn clean_temp_dir(store_base: &std::path::Path, dry_run: bool) -> u64 {
    let temp_dir = store_base.join("temp");
    if !temp_dir.exists() {
        return 0;
    }

    let mut removed = 0u64;
    let now = SystemTime::now();

    if let Ok(entries) = std::fs::read_dir(&temp_dir) {
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            if !entry.path().is_file() {
                continue;
            }
            let modified = match entry.metadata().and_then(|m| m.modified()) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let age = match now.duration_since(modified) {
                Ok(d) => d,
                Err(_) => continue,
            };
            if age > Duration::from_secs(3600) {
                if !dry_run {
                    let _ = std::fs::remove_file(entry.path());
                }
                removed += 1;
            }
        }
    }

    removed
}

fn collect_on_disk_hashes(objects_dir: &std::path::Path) -> HashSet<String> {
    let mut hashes = HashSet::new();
    for entry in walkdir::WalkDir::new(objects_dir).min_depth(3).max_depth(3) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.file_type().is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("sha256-") {
                hashes.insert(name);
            }
        }
    }
    hashes
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn setup_full() -> (tempfile::TempDir, Store, StoreIndex) {
        let dir = tempfile::TempDir::new().unwrap();
        let store_base = dir.path().join("store");
        let store = Store::new(store_base.clone());
        store.ensure_dirs().unwrap();
        let index = StoreIndex::new(store_base.join("index.db")).unwrap();
        (dir, store, index)
    }

    #[test]
    fn test_gc_clean_store() {
        let (dir, store, index) = setup_full();
        let store_base = dir.path().join("store");

        let hash = store.put(b"content").unwrap();
        index.insert("npm:pkg@1.0.0", &hash, "npm", 7).unwrap();

        // Actually create the extracted directory to simulate an extracted package
        let extracted_path = store.get_extracted_path(&hash);
        std::fs::create_dir_all(&extracted_path).unwrap();
        std::fs::write(extracted_path.join("index.js"), b"module.exports = {}").unwrap();
        index.increment_extracted(&hash).unwrap();

        let orphan = store.put(b"orphan").unwrap();
        let _ = store.put_graph(b"graph");

        cmd_gc_in(&store_base, false, false).unwrap();

        assert!(store.contains(&hash));
        assert!(store.get_extracted_path(&hash).exists());
        assert!(!store.contains(&orphan));
    }

    #[test]
    fn test_gc_no_index() {
        let dir = tempfile::TempDir::new().unwrap();
        let store_base = dir.path().join("store");
        let store = Store::new(store_base.clone());
        store.ensure_dirs().unwrap();

        store.put(b"some-data").unwrap();
        cmd_gc_in(&store_base, false, false).unwrap();
    }

    #[test]
    fn test_gc_dry_run_does_not_delete() {
        let (dir, store, index) = setup_full();
        let store_base = dir.path().join("store");

        let hash = store.put(b"keep-me").unwrap();
        index.insert("pkg@1", &hash, "npm", 7).unwrap();
        index.remove("pkg@1").unwrap();

        cmd_gc_in(&store_base, true, false).unwrap();
        assert!(store.contains(&hash));
    }

    #[test]
    fn test_gc_aggressive_removes_unreferenced() {
        let (dir, store, _index) = setup_full();
        let store_base = dir.path().join("store");

        store.put(b"unreferenced").unwrap();
        cmd_gc_in(&store_base, false, true).unwrap();
    }

    #[test]
    fn test_collect_on_disk_hashes_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let hashes = collect_on_disk_hashes(&dir.path().join("nonexistent"));
        assert!(hashes.is_empty());
    }

    #[test]
    fn test_stale_temp_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let temp_dir = dir.path().join("temp");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let cleaned = clean_temp_dir(dir.path(), false);
        assert_eq!(cleaned, 0);
    }
}
