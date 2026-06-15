use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};

use ara_lockfile::types::PackageEntry;
use ara_store::cas::Store;
use ara_store::index::StoreIndex;
use ara_types::{Constraint, PackageIdentity, SourceType, Version};

use super::disk_ops;

/// Extract and sort all version strings from package metadata.
/// Compact extracted metadata: only versions + dependency maps + integrity,
/// avoids storing the full npm registry response (30MB+ for next.js).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct PackageMeta {
    versions: Vec<String>,
    deps: HashMap<String, HashMap<String, String>>,
    #[serde(default)]
    integrity: HashMap<String, String>,
}

fn registry_cache_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".ara")
            .join("cache")
            .join("registry"),
    )
}

fn safe_cache_name(name: &str) -> String {
    let name = name.replace('/', "_").replace('@', "");
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn read_package_meta(name: &str) -> Option<PackageMeta> {
    let dir = registry_cache_dir()?;
    let safe_name = safe_cache_name(name);
    let path = dir.join(format!("{safe_name}.json"));
    if !path.exists() {
        return None;
    }
    let metadata = std::fs::metadata(&path).ok()?;
    if let Ok(modified) = metadata.modified() {
        if let Ok(elapsed) = modified.elapsed() {
            if elapsed > Duration::from_secs(604800) {
                let _ = std::fs::remove_file(&path);
                return None;
            }
        }
    }
    let file = std::fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);
    serde_json::from_reader(reader).ok()
}

fn write_package_meta(name: &str, meta: &PackageMeta) {
    let dir = match registry_cache_dir() {
        Some(d) => d,
        None => return,
    };
    let safe_name = safe_cache_name(name);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("  warning: failed to create registry cache dir: {e}");
        return;
    }
    let path = dir.join(format!("{safe_name}.json"));
    let file = match std::fs::File::create(&path) {
        Ok(f) => f,
        Err(_) => return,
    };
    let writer = std::io::BufWriter::new(file);
    let _ = serde_json::to_writer(writer, meta);
}

/// Extract sorted versions + dependency map + integrity from full npm metadata JSON.
fn extract_package_meta(meta: &serde_json::Value) -> PackageMeta {
    let versions_map = meta["versions"].as_object().cloned().unwrap_or_default();
    let mut versions: Vec<String> = versions_map.keys().cloned().collect();
    versions.sort_by(|a, b| {
        let va = Version::parse(a);
        let vb = Version::parse(b);
        match (va, vb) {
            (Ok(va), Ok(vb)) => va.cmp(&vb),
            _ => a.cmp(b),
        }
    });
    let mut deps: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut integrity: HashMap<String, String> = HashMap::new();
    for (ver_str, ver_data) in &versions_map {
        let mut deps_map = HashMap::new();
        if let Some(dep_map) = ver_data["dependencies"].as_object() {
            for (k, v) in dep_map {
                if let Some(s) = v.as_str() {
                    deps_map.insert(k.clone(), s.to_string());
                }
            }
        }
        if let Some(opt_map) = ver_data["optionalDependencies"].as_object() {
            for (k, v) in opt_map {
                if let Some(s) = v.as_str() {
                    deps_map.insert(k.clone(), s.to_string());
                }
            }
        }
        if !deps_map.is_empty() {
            deps.insert(ver_str.clone(), deps_map);
        }
        // Extract integrity hash from dist metadata (npm registry)
        let dist = &ver_data["dist"];
        let hash = dist["integrity"]
            .as_str()
            .or_else(|| dist["shasum"].as_str())
            .map(|s| s.to_string());
        if let Some(h) = hash {
            integrity.insert(ver_str.clone(), h);
        }
    }
    PackageMeta {
        versions,
        deps,
        integrity,
    }
}

pub(crate) async fn install_transitive_deps(
    node_modules: &Path,
    store: &Store,
    store_index: &Arc<StoreIndex>,
    pkg_entries: &mut Vec<PackageEntry>,
    initial_packages: &[String],
) -> Result<()> {
    let registry_url = std::env::var("ARA_NPM_REGISTRY")
        .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());
    let reg = ara_source::registry::RegistrySource::new(registry_url)?;
    // Pre-warm the HTTP/2 connection so the thundering herd of
    // concurrent downloads multiplexes over a single connection
    // instead of each opening its own TCP + TLS handshake.
    reg.warmup().await;

    // Pre-populate resolution from packages already known to be installed
    let mut resolution: HashMap<String, String> = HashMap::new();
    for entry in &*pkg_entries {
        resolution.insert(entry.name.clone(), entry.version.clone());
    }

    // Seed the queue: initial packages + pre-existing node_modules
    // These go into pending so their deps can be scanned from metadata.
    let existing = disk_ops::collect_installed_names(node_modules);
    let mut seen: HashSet<String> = HashSet::new();
    let mut pending: Vec<String> = initial_packages
        .iter()
        .chain(existing.iter())
        .filter(|n| !n.is_empty() && seen.insert(n.to_string()))
        .cloned()
        .collect();

    // Uses compact PackageMeta (versions + deps only) instead of storing
    // the full npm registry response (30MB+ for next.js).
    let t_resolution = Instant::now();
    let mut level_count = 0u32;
    let mut meta_cache: HashMap<String, PackageMeta> = HashMap::new();
    let mut t_fetch = Duration::ZERO;
    let mut t_resolve = Duration::ZERO;
    while !pending.is_empty() {
        level_count += 1;
        let batch: Vec<String> = std::mem::take(&mut pending);

        let batch_uncached: Vec<String> = batch
            .iter()
            .filter(|n| !meta_cache.contains_key(*n))
            .cloned()
            .collect();
        if !batch_uncached.is_empty() {
            let t0 = Instant::now();
            let mut tasks = Vec::new();
            for name in batch_uncached {
                let name = name.clone();
                let reg = reg.clone();
                let exact_ver_exists = resolution.contains_key(&name);
                tasks.push(tokio::spawn(async move {
                    if !exact_ver_exists {
                        return None;
                    }
                    match tokio::task::spawn_blocking({
                        let n = name.clone();
                        move || read_package_meta(&n)
                    })
                    .await
                    {
                        Ok(Some(pm)) => return Some((name, pm)),
                        Ok(None) => {}
                        Err(e) => {
                            eprintln!("  warning: failed to read cached metadata for {name} (task panic): {e}");
                        }
                    }
                    if let Ok(meta) = reg.fetch_metadata(&name).await {
                        let pm = extract_package_meta(&meta);
                        let _ = tokio::task::spawn_blocking({
                            let n = name.clone();
                            let p = pm.clone();
                            move || write_package_meta(&n, &p)
                        })
                        .await;
                        return Some((name, pm));
                    }
                    None
                }));
            }
            let fetched: Vec<(String, PackageMeta)> = futures::future::join_all(tasks)
                .await
                .into_iter()
                .filter_map(|x| match x {
                    Ok(inner) => inner,
                    Err(e) => {
                        eprintln!("  warning: metadata fetch task panic: {e}");
                        None
                    }
                })
                .collect();
            t_fetch += t0.elapsed();
            for (name, pm) in fetched {
                meta_cache.insert(name, pm);
            }
        }

        let dep_lists: Vec<Vec<(String, String)>> = batch
            .iter()
            .filter_map(|name| {
                let exact_ver = resolution.get(name)?;
                let pm = meta_cache.get(name)?;
                let deps = pm.deps.get(exact_ver).cloned().unwrap_or_default();
                let all: Vec<(String, String)> = deps.into_iter().collect();
                if all.is_empty() {
                    None
                } else {
                    Some(all)
                }
            })
            .collect();

        let mut seen_dep: HashSet<String> = HashSet::new();
        let unique_deps: Vec<(String, String)> = dep_lists
            .iter()
            .flatten()
            .filter(|(name, _)| seen_dep.insert(name.clone()))
            .filter(|(name, _)| {
                !resolution.contains_key(name.as_str())
                    && !node_modules.join(name.as_str()).exists()
            })
            .cloned()
            .collect();

        let dep_uncached: Vec<String> = unique_deps
            .iter()
            .map(|(name, _)| name.clone())
            .filter(|n| !meta_cache.contains_key(n))
            .collect();
        if !dep_uncached.is_empty() {
            let t0 = Instant::now();
            let mut tasks = Vec::new();
            for name in dep_uncached {
                let name = name.clone();
                let reg = reg.clone();
                tasks.push(tokio::spawn(async move {
                    match tokio::task::spawn_blocking({
                        let n = name.clone();
                        move || read_package_meta(&n)
                    })
                    .await
                    {
                        Ok(Some(pm)) => return Some((name, pm)),
                        Ok(None) => {}
                        Err(e) => {
                            eprintln!("  warning: failed to read cached metadata for {name} (task panic): {e}");
                        }
                    }
                    if let Ok(meta) = reg.fetch_metadata(&name).await {
                        let pm = extract_package_meta(&meta);
                        let _ = tokio::task::spawn_blocking({
                            let n = name.clone();
                            let p = pm.clone();
                            move || write_package_meta(&n, &p)
                        })
                        .await;
                        return Some((name, pm));
                    }
                    None
                }));
            }
            let fetched: Vec<(String, PackageMeta)> = futures::future::join_all(tasks)
                .await
                .into_iter()
                .filter_map(|x| match x {
                    Ok(inner) => inner,
                    Err(e) => {
                        eprintln!("  warning: metadata fetch task panic: {e}");
                        None
                    }
                })
                .collect();
            t_fetch += t0.elapsed();
            for (name, pm) in fetched {
                meta_cache.insert(name, pm);
            }
        }

        let t1 = Instant::now();
        let discovered: Vec<(String, String)> = unique_deps
            .iter()
            .filter_map(|(dep_name, dep_range)| {
                let pm = meta_cache.get(dep_name)?;
                let constraint = match Constraint::parse(dep_range) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("  warning: failed to parse constraint '{dep_range}' for {dep_name}: {e}");
                        return None;
                    }
                };
                // Versions are sorted ascending, find highest matching
                let best = pm
                    .versions
                    .iter()
                    .rev()
                    .filter_map(|v| {
                        Version::parse(v)
                            .ok()
                            .filter(|pv| constraint.satisfied_by(pv))
                    })
                    .next()?;
                Some((dep_name.clone(), best.to_string()))
            })
            .collect();
        t_resolve += t1.elapsed();

        for (name, ver) in discovered {
            if seen.insert(name.clone()) {
                resolution.insert(name.clone(), ver);
                pending.push(name);
            }
        }

        eprintln!("  resolved {} packages...", resolution.len());
    }
    eprintln!(
        "  [profile] phase 3a resolution ({} levels, {} pkgs): {:?} (fetch {:.3}s, resolve {:.3}s)",
        level_count,
        resolution.len(),
        t_resolution.elapsed(),
        t_fetch.as_secs_f64(),
        t_resolve.as_secs_f64(),
    );

    // Download -> extract -> bin links (I/O pool, 32 threads)
    let t_dle = Instant::now();
    let cache_hits = Arc::new(AtomicU32::new(0));
    let initial_pkgs: HashSet<String> = pkg_entries.iter().map(|e| e.name.clone()).collect();
    let mut total_tasks = 0usize;
    // Collect fresh download entries for batch SQLite insert — avoids
    // N concurrent tasks fighting over the StoreIndex mutex.
    type IndexPending = Arc<std::sync::Mutex<Vec<(String, String, String, i64)>>>;
    let index_pending: IndexPending = Arc::new(std::sync::Mutex::new(Vec::new()));
    // Debug counters for cumulative time breakdown
    let total_fetch_us: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let total_extract_us: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Option<(String, String, String)>>(1024);
    let idx_store = Arc::clone(store_index);
    let nm = node_modules;
    let cache_hits_ref = Arc::clone(&cache_hits);
    let mut tasks = Vec::new();
    // Pre-compute content hashes from metadata for integrity verification
    let content_hashes: HashMap<String, Option<String>> = resolution
        .iter()
        .map(|(name, ver)| {
            let hash = meta_cache
                .get(name)
                .and_then(|pm| pm.integrity.get(ver))
                .cloned();
            (name.clone(), hash)
        })
        .collect();
    for (dep_name, exact_ver) in &resolution {
        if initial_pkgs.contains(dep_name) {
            continue;
        }
        let parsed_ver = match Version::parse(exact_ver) {
            Ok(v) => v,
            Err(_) => continue,
        };
        total_tasks += 1;
        let short_ver = format!(
            "{}.{}.{}",
            parsed_ver.major, parsed_ver.minor, parsed_ver.patch
        );
        let cache_key = format!("npm:{}@{}", dep_name, short_ver);
        let dep_name = dep_name.clone();
        let tx = tx.clone();
        let idx = Arc::clone(&idx_store);
        let ch = Arc::clone(&cache_hits_ref);
        let idx_pending = Arc::clone(&index_pending);
        let tf = Arc::clone(&total_fetch_us);
        let te = Arc::clone(&total_extract_us);
        let reg_ref = reg.clone();
        let store_ref = store.clone();
        let nm = nm.to_path_buf();
        let content_hash = content_hashes.get(&dep_name).and_then(|h| h.clone());

        tasks.push(tokio::spawn(async move {
            let dep_dir = nm.join(&dep_name);
            if dep_dir.exists() {
                let hash_str = tokio::task::spawn_blocking({
                    let idx = idx.clone();
                    let ck = cache_key.clone();
                    move || idx.lookup(&ck).ok().flatten().unwrap_or_default()
                })
                .await
                .unwrap_or_default();
                let _ = tx.send(Some((dep_name, short_ver, hash_str))).await;
                return;
            }

            let identity = PackageIdentity {
                source: SourceType::Npm,
                name: dep_name.clone(),
                version: parsed_ver.clone(),
                content_hash,
                requested_ref: None,
            };

            // Quick synchronous cache check
            let cache_lookup = tokio::task::spawn_blocking({
                let idx = idx.clone();
                let ck = cache_key.clone();
                let s = store_ref.clone();
                move || {
                    let hash = idx.lookup(&ck).ok().flatten()?;
                    if s.contains(&hash) {
                        Some(hash)
                    } else {
                        None
                    }
                }
            })
            .await
            .ok()
            .flatten();

            if let Some(cached_hash) = cache_lookup {
                // Cache hit: just extract + bin links in one shot
                ch.fetch_add(1, Ordering::Relaxed);
                let result_clone = cached_hash.clone();
                let s = store_ref.clone();
                let nm_c = nm.clone();
                let dn = dep_name.clone();
                let dd = dep_dir.clone();
                let extraction = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
                    disk_ops::extract_package_cached(&s, &result_clone, &dd, None)?;
                    let _ = disk_ops::install_bin_links(&nm_c, &dn, &dd);
                    Ok(())
                })
                .await;
                match extraction {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        eprintln!("    failed to extract {}: {}", dep_name, e);
                        let _ = tx.send(None).await;
                        return;
                    }
                    Err(e) => {
                        eprintln!("    failed to extract {} (task panic): {}", dep_name, e);
                        let _ = tx.send(None).await;
                        return;
                    }
                }
                let _ = tx.send(Some((dep_name, short_ver, cached_hash))).await;
                return;
            }

            // Fresh download: fetch over network, then do ALL disk I/O in one spawn_blocking
            let t_fetch = Instant::now();
            let content = match reg_ref.fetch(&identity).await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("    warning: failed to fetch {}: {}", dep_name, e);
                    let _ = tx.send(None).await;
                    return;
                }
            };
            tf.fetch_add(t_fetch.elapsed().as_micros() as u64, Ordering::Relaxed);

            let content_size = content.len() as i64;
            let dn = dep_name.clone();
            let s = store_ref.clone();
            let nm_c = nm.clone();
            let dd = dep_dir.clone();
            let t_extract = Instant::now();
            let result = tokio::task::spawn_blocking(move || -> Option<String> {
                // hash + store tarball
                let hash = match s.put(&content) {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("    warning: failed to store {}: {}", dn, e);
                        return None;
                    }
                };
                // extract directly from content (no disk re-read!)
                let extracted_dir = s.get_extracted_path(&hash);
                if !extracted_dir.exists() {
                    if let Err(e) = std::fs::create_dir_all(&extracted_dir) {
                        eprintln!("    failed to create extract dir for {}: {}", dn, e);
                        return Some(hash);
                    }
                    if let Err(e) = disk_ops::extract_tarball(&content, &extracted_dir) {
                        eprintln!("    failed to extract {}: {}", dn, e);
                        return Some(hash);
                    }
                }
                drop(content); // free memory before hardlinking
                               // 3. Hardlink to node_modules
                let _ = std::fs::remove_dir_all(&dd);
                if let Err(e) = disk_ops::hardlink_dir(&extracted_dir, &dd) {
                    eprintln!("    failed to hardlink {}: {}", dn, e);
                }
                // install bin links
                let _ = disk_ops::install_bin_links(&nm_c, &dn, &dd);
                Some(hash)
            })
            .await;
            te.fetch_add(t_extract.elapsed().as_micros() as u64, Ordering::Relaxed);

            match result {
                Ok(Some(hash)) => {
                    idx_pending.lock().unwrap_or_else(|e| e.into_inner()).push((
                        cache_key,
                        hash.clone(),
                        "npm".to_string(),
                        content_size,
                    ));
                    let _ = tx.send(Some((dep_name, short_ver, hash))).await;
                }
                Ok(None) => {
                    eprintln!("    warning: skipped {} (store failed)", dep_name);
                    let _ = tx.send(None).await;
                }
                Err(e) => {
                    eprintln!("    failed {} (task panic): {}", dep_name, e);
                    let _ = tx.send(None).await;
                }
            }
        }));
    }
    drop(tx);

    let pb = ProgressBar::new(total_tasks as u64);
    pb.set_style(
        ProgressStyle::with_template("[{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .context("invalid progress bar template")?
            .progress_chars("##-"),
    );
    let mut results: Vec<(String, String, String)> = Vec::new();
    while let Some(item) = rx.recv().await {
        if let Some(entry) = item {
            results.push(entry);
        }
        pb.inc(1);
    }
    pb.finish_and_clear();
    let fresh = total_tasks.saturating_sub(cache_hits.load(Ordering::Relaxed) as usize);
    let fetch_s = total_fetch_us.load(Ordering::Relaxed) as f64 / 1_000_000.0;
    let extract_s = total_extract_us.load(Ordering::Relaxed) as f64 / 1_000_000.0;
    eprintln!(
        "  [profile] phase 3b download+extract: {:?} (hits: {}, fresh: {}, fetch: {:.3}s, extract: {:.3}s)",
        t_dle.elapsed(),
        cache_hits.load(Ordering::Relaxed),
        fresh,
        fetch_s,
        extract_s,
    );

    // Batch insert index entries — single transaction instead of N
    // concurrent inserts contending for the SQLite mutex.
    {
        let pending = index_pending.lock().unwrap_or_else(|e| e.into_inner());
        if !pending.is_empty() {
            if let Err(e) = store_index.batch_insert(&pending) {
                eprintln!("  warning: failed to batch cache index entries: {e}");
            }
        }
    }

    for (dep_name, short_ver, hash_str) in &results {
        let integrity = content_hashes.get(dep_name).and_then(|h| h.clone());
        pkg_entries.push(PackageEntry {
            name: dep_name.clone(),
            version: short_ver.clone(),
            source: "npm".to_string(),
            package_hash: hash_str.clone(),
            integrity,
            signature: None,
            repository: None,
            commit: None,
            dependencies: None,
            security: None,
            sbom: None,
        });
    }

    Ok(())
}
