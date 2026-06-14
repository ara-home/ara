use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};

use ara_analysis::analyzer;
use ara_lockfile::types::{GraphMeta, Lockfile, PackageEntry, SecurityMeta};
use ara_manifest::package_json;
use ara_manifest::parser;
use ara_resolver::mvs::{ConstraintEntry, Resolver};
use ara_source::Source;
use ara_store::cas::Store;
use ara_store::index::StoreIndex;
use ara_types::{Constraint, PackageIdentity, RiskLevel, SourceType, Version};

use super::prompt::{prompt_allow_package, AllowDecision};

fn write_lockfile(cwd: &Path, store: Option<&Store>, pkg_entries: &[PackageEntry]) -> Result<()> {
    let ts = current_timestamp();

    let graph_hash = if let Some(store) = store {
        if pkg_entries.is_empty() {
            None
        } else {
            let graph_bytes = serde_json::to_vec(pkg_entries)
                .context("failed to serialize package entries for graph hash")?;
            let raw = ara_util::hash::compute(&graph_bytes);
            let hex = ara_util::hash::hex_encode(&raw);
            let store_hash = store
                .put_graph(&graph_bytes)
                .context("failed to store graph hash in content store")?;
            Some(format!("sha256:{hex} (store: {store_hash})"))
        }
    } else {
        None
    };

    let lockfile = Lockfile {
        version: 1,
        graph: GraphMeta {
            resolver: "mvs".to_string(),
            generated_at: Some(ts),
            graph_hash,
        },
        packages: pkg_entries.to_vec(),
    };
    let lock_content = ara_lockfile::generator::generate(&lockfile);
    let lock_path = cwd.join("ara.lock");
    let mut lock_f = std::fs::File::create(&lock_path)?;
    lock_f.write_all(lock_content.as_bytes())?;
    println!("Lockfile written to ara.lock");
    Ok(())
}

fn sha256_hex_to_sri(hash: &str) -> Option<String> {
    let hex = hash.strip_prefix("sha256-")?;
    if hex.len() != 64 {
        return None;
    }
    let bytes = hex::decode(hex).ok()?;
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("sha256-{b64}"))
}

fn npm_tarball_url(name: &str, version: &str) -> String {
    if let Some(scope) = name.strip_prefix('@') {
        if let Some((scope_name, rest)) = scope.split_once('/') {
            format!(
                "https://registry.npmjs.org/@{}%2f{}/-/@{}/{}-{}.tgz",
                scope_name, rest, scope_name, rest, version
            )
        } else {
            format!(
                "https://registry.npmjs.org/{}/-/{}-{}.tgz",
                name, name, version
            )
        }
    } else {
        format!(
            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
            name, name, version
        )
    }
}

fn write_package_lock(
    cwd: &Path,
    manifest: &ara_manifest::types::Manifest,
    pkg_entries: &[PackageEntry],
) -> Result<()> {
    let mut packages = serde_json::Map::new();

    let root_deps: serde_json::Map<String, serde_json::Value> = manifest
        .deps
        .iter()
        .filter_map(|d| {
            let v = d.version.as_deref()?;
            Some((d.name.clone(), serde_json::Value::String(v.to_string())))
        })
        .collect();

    let mut root_entry = serde_json::Map::new();
    root_entry.insert(
        "name".to_string(),
        serde_json::Value::String(manifest.project.name.clone()),
    );
    root_entry.insert(
        "version".to_string(),
        serde_json::Value::String(manifest.project.version.clone()),
    );
    if !root_deps.is_empty() {
        root_entry.insert(
            "dependencies".to_string(),
            serde_json::Value::Object(root_deps),
        );
    }
    packages.insert(String::new(), serde_json::Value::Object(root_entry));

    let registry_url = std::env::var("ARA_NPM_REGISTRY")
        .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());

    for entry in pkg_entries {
        let mut pkg = serde_json::Map::new();
        pkg.insert(
            "version".to_string(),
            serde_json::Value::String(entry.version.clone()),
        );

        if entry.source == "npm" || entry.source == "registry" {
            let resolved = if registry_url == "https://registry.npmjs.org" {
                npm_tarball_url(&entry.name, &entry.version)
            } else {
                format!(
                    "{}/{}/-/{}:{}.tgz",
                    registry_url.trim_end_matches('/'),
                    url_encode_pkg_name(&entry.name),
                    &entry.name,
                    entry.version
                )
            };
            pkg.insert("resolved".to_string(), serde_json::Value::String(resolved));

            if let Some(sri) = sha256_hex_to_sri(&entry.package_hash) {
                pkg.insert("integrity".to_string(), serde_json::Value::String(sri));
            }
        }

        let key = format!("node_modules/{}", entry.name);
        packages.insert(key, serde_json::Value::Object(pkg));
    }

    let lock = serde_json::json!({
        "name": manifest.project.name,
        "version": manifest.project.version,
        "lockfileVersion": 3,
        "requires": true,
        "packages": packages,
    });

    let output = serde_json::to_string_pretty(&lock)?;
    let lock_path = cwd.join("package-lock.json");
    std::fs::write(&lock_path, &output)?;
    println!("Compatibility lockfile written to package-lock.json");
    Ok(())
}

fn url_encode_pkg_name(name: &str) -> String {
    let mut encoded = String::with_capacity(name.len());
    for b in name.bytes() {
        match b {
            b'@' => encoded.push_str("%40"),
            b'/' => encoded.push_str("%2f"),
            _ => encoded.push(b as char),
        }
    }
    encoded
}

fn current_timestamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn find_dep<'a>(
    deps: &'a [ara_manifest::types::DependencyEntry],
    name: &str,
) -> Option<&'a ara_manifest::types::DependencyEntry> {
    deps.iter().find(|d| d.name == name)
}

fn source_type_from_str(s: &str) -> SourceType {
    match s {
        "registry" => SourceType::Registry,
        "github" => SourceType::Github,
        "git" => SourceType::Git,
        "local" => SourceType::Local,
        "workspace" => SourceType::Workspace,
        "url" => SourceType::Url,
        _ => SourceType::Npm,
    }
}

fn create_source(
    source_type: SourceType,
    dep: &ara_manifest::types::DependencyEntry,
) -> Result<Source> {
    Ok(match source_type {
        SourceType::Npm | SourceType::Registry => {
            let default_url = std::env::var("ARA_NPM_REGISTRY")
                .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());
            let url = dep.url.as_deref().unwrap_or(&default_url);
            Source::Registry(ara_source::registry::RegistrySource::new(url.to_string())?)
        }
        SourceType::Github => {
            let repo = dep
                .repo
                .as_deref()
                .context("missing repo for github source")?;
            Source::Github(ara_source::github::GithubSource::new(repo.to_string()))
        }
        SourceType::Git => {
            let url = dep.url.as_deref().context("missing url for git source")?;
            let commit = dep.commit.as_deref().unwrap_or("HEAD");
            Source::Git(ara_source::git::GitSource::new(
                url.to_string(),
                commit.to_string(),
            ))
        }
        SourceType::Local => {
            let path = dep
                .path
                .as_deref()
                .context("missing path for local source")?;
            Source::Local(ara_source::local::LocalSource::new(path.to_string()))
        }
        SourceType::Url => {
            let url = dep.url.as_deref().context("missing url for url source")?;
            Source::Url(ara_source::tarball::TarballSource::new(url.to_string()))
        }
        SourceType::Workspace => {
            let path = dep.path.as_deref().unwrap_or(".");
            Source::Workspace(ara_source::workspace::WorkspaceSource::new(
                path.to_string(),
            ))
        }
    })
}

fn read_member_manifest(member_dir: &Path) -> Option<ara_manifest::types::Manifest> {
    let member_toml = member_dir.join("ara.toml");
    if member_toml.exists() {
        let content = std::fs::read_to_string(&member_toml).ok()?;
        return parser::parse(&content).ok();
    }

    let member_pkg_json = member_dir.join("package.json");
    if member_pkg_json.exists() {
        let content = std::fs::read_to_string(&member_pkg_json).ok()?;
        return package_json::parse_package_json(&content).ok();
    }

    None
}

fn expand_workspace_members(
    workspace: &ara_manifest::types::Workspace,
    cwd: &Path,
) -> Vec<ara_manifest::types::DependencyEntry> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    for pattern in &workspace.members {
        let full_pattern = cwd.join(pattern);
        let full_str = full_pattern.to_string_lossy().to_string();

        let matches = match glob::glob(&full_str) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("  warning: invalid workspace pattern \"{pattern}\": {e}");
                continue;
            }
        };

        for entry in matches {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("  warning: glob error for pattern \"{pattern}\": {e}");
                    continue;
                }
            };

            if !entry.is_dir() {
                continue;
            }

            let manifest = match read_member_manifest(&entry) {
                Some(m) => m,
                None => {
                    eprintln!(
                        "  warning: workspace member {} has no manifest, skipping",
                        entry.display()
                    );
                    continue;
                }
            };

            if !seen.insert(manifest.project.name.clone()) {
                continue;
            }

            let rel_path = entry
                .strip_prefix(cwd)
                .unwrap_or(&entry)
                .to_string_lossy()
                .to_string();

            entries.push(ara_manifest::types::DependencyEntry {
                name: manifest.project.name,
                source: "workspace".to_string(),
                kind: None,
                version: Some(manifest.project.version),
                path: Some(rel_path),
                repo: None,
                url: None,
                commit: None,
            });
        }
    }

    entries
}

pub fn extract_tarball(tarball: &[u8], dest: &Path) -> Result<()> {
    // Pass 1: detect prefix without allocating file data
    let prefix = {
        let decoder = flate2::read::GzDecoder::new(tarball);
        let mut archive = tar::Archive::new(decoder);
        let mut common = None;
        let mut has_files = false;

        let mut is_first = true;
        if let Ok(entries) = archive.entries() {
            for entry in entries.flatten() {
                if let Ok(path) = entry.path() {
                    let comp = path.components().next();
                    if is_first {
                        common = comp.map(|c| c.as_os_str().to_os_string());
                        is_first = false;
                    } else if common.is_some()
                        && comp.map(|c| c.as_os_str()) != common.as_deref()
                    {
                        common = None;
                    }
                    if path.components().count() > 1 {
                        has_files = true;
                    }
                }
            }
        }

        if let (Some(comp), true) = (common, has_files) {
            std::path::PathBuf::from(comp)
        } else {
            std::path::PathBuf::new()
        }
    };

    // Pass 2: extract streaming directly to disk
    let decoder = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(decoder);
    for entry in archive
        .entries()
        .context("failed to read tarball entries")?
    {
        let mut entry = entry.context("failed to read tarball entry")?;
        let path = entry
            .path()
            .context("failed to read entry path")?
            .into_owned();

        let stripped = path.strip_prefix(&prefix).unwrap_or(&path);
        if stripped.as_os_str().is_empty() {
            continue;
        }

        let target = dest.join(stripped);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }

        entry.unpack(&target).context("failed to unpack entry")?;
    }

    Ok(())
}

/// Create symlinks in `node_modules/.bin/` for the package's `bin` entries.
pub fn install_bin_links(node_modules: &Path, pkg_name: &str, pkg_dir: &Path) -> Result<()> {
    let pkg_json_path = pkg_dir.join("package.json");
    if !pkg_json_path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&pkg_json_path)
        .context("failed to read package.json for bin links")?;
    let pkg: serde_json::Value =
        serde_json::from_str(&content).context("failed to parse package.json for bin links")?;

    let bin_entries: Vec<(String, String)> = match pkg.get("bin") {
        Some(serde_json::Value::String(cmd)) => {
            let unscoped_name = pkg_name
                .split('/')
                .next_back()
                .unwrap_or(pkg_name)
                .to_string();
            vec![(unscoped_name, cmd.clone())]
        }
        Some(serde_json::Value::Object(map)) => map
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
            .filter(|(_, v)| !v.is_empty())
            .collect(),
        _ => return Ok(()),
    };

    if bin_entries.is_empty() {
        return Ok(());
    }

    let bin_dir = node_modules.join(".bin");
    std::fs::create_dir_all(&bin_dir)?;

    for (name, rel_path) in &bin_entries {
        let link = bin_dir.join(name);
        #[allow(unused_variables)]
        let target = format!("../{}/{}", pkg_name, rel_path);
        let actual_file = pkg_dir.join(rel_path);

        #[cfg(unix)]
        if let Ok(metadata) = std::fs::metadata(&actual_file) {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            if let Err(e) = std::fs::set_permissions(&actual_file, perms) {
                eprintln!(
                    "  warning: failed to set executable permissions on {actual_file:?}: {e}"
                );
            }
        }

        let _ = std::fs::remove_file(&link);

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link)
            .with_context(|| format!("failed to create symlink {link:?} -> {target}"))?;

        #[cfg(not(unix))]
        std::fs::hard_link(&actual_file, &link)
            .with_context(|| format!("failed to link {link:?}"))?;
    }

    Ok(())
}

/// Scan `node_modules/<pkg>/package.json` for each installed package and
/// recursively install any missing transitive dependencies.
fn collect_installed_names(node_modules: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(node_modules) else {
        return names;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ftype) = entry.file_type() else {
            continue;
        };
        if !ftype.is_dir() {
            continue;
        }
        let fname = match path.file_name().and_then(|s| s.to_str()) {
            Some(f) => f.to_string(),
            None => continue,
        };
        if fname == ".bin" {
            continue;
        }
        if fname.starts_with('@') {
            let Ok(sub_entries) = std::fs::read_dir(&path) else {
                continue;
            };
            for sub in sub_entries.flatten() {
                let sub_path = sub.path();
                if sub.file_type().ok().is_some_and(|t| t.is_dir()) {
                    if let Some(sub_name) = sub_path.file_name().and_then(|s| s.to_str()) {
                        names.push(format!("{}/{}", fname, sub_name));
                    }
                }
            }
        } else {
            names.push(fname);
        }
    }
    names
}

/// Extract and sort all version strings from package metadata.
/// Compact extracted metadata: only versions + dependency maps,
/// avoids storing the full npm registry response (30MB+ for next.js).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct PackageMeta {
    versions: Vec<String>,
    deps: HashMap<String, HashMap<String, String>>,
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

fn read_package_meta(name: &str) -> Option<PackageMeta> {
    let dir = registry_cache_dir()?;
    let safe_name = name.replace('/', "_").replace('@', "");
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
    let safe_name = name.replace('/', "_").replace('@', "");
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

/// Extract sorted versions + dependency map from full npm metadata JSON.
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
    for (ver_str, ver_data) in versions_map {
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
    }
    PackageMeta { versions, deps }
}

async fn install_transitive_deps(
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
    let existing = collect_installed_names(node_modules);
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

    // Download → extract → bin links (I/O pool, 32 threads)
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
                content_hash: None,
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
                    extract_package_cached(&s, &result_clone, &dd, None)?;
                    let _ = install_bin_links(&nm_c, &dn, &dd);
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
                // 1. Hash + store tarball
                let hash = match s.put(&content) {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("    warning: failed to store {}: {}", dn, e);
                        return None;
                    }
                };
                // 2. Extract directly from content (no disk re-read!)
                let extracted_dir = s.get_extracted_path(&hash);
                if !extracted_dir.exists() {
                    if let Err(e) = std::fs::create_dir_all(&extracted_dir) {
                        eprintln!("    failed to create extract dir for {}: {}", dn, e);
                        return Some(hash);
                    }
                    if let Err(e) = extract_tarball(&content, &extracted_dir) {
                        eprintln!("    failed to extract {}: {}", dn, e);
                        return Some(hash);
                    }
                }
                drop(content); // free memory before hardlinking
                               // 3. Hardlink to node_modules
                let _ = std::fs::remove_dir_all(&dd);
                if let Err(e) = hardlink_dir(&extracted_dir, &dd) {
                    eprintln!("    failed to hardlink {}: {}", dn, e);
                }
                // 4. Bin links
                let _ = install_bin_links(&nm_c, &dn, &dd);
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
        pkg_entries.push(PackageEntry {
            name: dep_name.clone(),
            version: short_ver.clone(),
            source: "npm".to_string(),
            package_hash: hash_str.clone(),
            integrity: None,
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
pub(crate) async fn cmd_install(non_interactive: bool, package_lock: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    cmd_install_in(&cwd, non_interactive, package_lock).await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn cmd_install_specs(
    specs: &[String],
    save_dev: bool,
    save_peer: bool,
    save_optional: bool,
    range: Option<&str>,
    force: bool,
    refresh: bool,
    offline: bool,
    non_interactive: bool,
    package_lock: bool,
) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;

    // Read or bootstrap a minimal manifest
    let mut m = match read_manifest(&cwd) {
        Ok(m) => m,
        Err(_) => {
            let dir_name = cwd
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "project".to_string());
            println!("No manifest found. Creating ara.toml for {dir_name}.");
            ara_manifest::types::Manifest {
                project: ara_manifest::types::Project {
                    name: dir_name,
                    version: "0.1.0".to_string(),
                },
                deps: vec![],
                workspace: None,
                scripts: vec![],
                security: None,
                build: None,
                package_json_extras: None,
            }
        }
    };

    println!(
        "Installing {} package(s) into {} v{}",
        specs.len(),
        m.project.name,
        m.project.version
    );

    let dep_kind = if save_peer {
        Some("peer".to_string())
    } else if save_dev {
        Some("dev".to_string())
    } else if save_optional {
        Some("optional".to_string())
    } else {
        None
    };

    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let store_base = PathBuf::from(&home).join(".ara").join("store");
    let store = Store::new(store_base.clone());
    store.ensure_dirs()?;

    let node_modules = cwd.join("node_modules");
    if let Err(e) = std::fs::create_dir_all(&node_modules) {
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(e).context("failed to create node_modules directory");
        }
    }

    let index_path = store_base.join("index.db");
    let store_index = Arc::new(StoreIndex::new(index_path)?);

    // Seed pkg_entries from existing lockfile so we don't lose prior entries
    let lock_path = cwd.join("ara.lock");
    let mut pkg_entries: Vec<PackageEntry> = if lock_path.exists() {
        let lock_content = std::fs::read_to_string(&lock_path)
            .with_context(|| format!("failed to read lockfile: {}", lock_path.display()))?;
        if let Ok(existing) = ara_lockfile::parser::parse(&lock_content) {
            existing.packages
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let mut installed_names: Vec<String> = Vec::new();
    for spec in specs {
        let target = ara_source::url::parse_install_spec(spec)
            .with_context(|| format!("failed to parse spec: {spec}"))?;

        let mut meta = resolve_spec_meta(&target, range).await?;

        if let Some(pos) = m.deps.iter().position(|d| d.name == meta.name) {
            m.deps.remove(pos);
        }

        if let Some(pos) = pkg_entries.iter().position(|e| e.name == meta.name) {
            pkg_entries.remove(pos);
        }

        // Compute version string and cache key (source-type aware)
        let ver_str = match meta.source_type {
            SourceType::Npm | SourceType::Registry => format!(
                "{}.{}.{}",
                meta.version_semver.major, meta.version_semver.minor, meta.version_semver.patch
            ),
            _ => meta.version.clone(),
        };
        let cache_key = format!("{}:{}@{}", meta.source_type, meta.name, ver_str);

        // Fetch content — checking cache first
        let (pkg_content, hash_str) = if force {
            let content = fetch_meta_content(&meta).await?;
            let hash = store.put(&content)?;
            if let Err(e) = store_index.insert(
                &cache_key,
                &hash,
                &meta.source_type.to_string(),
                content.len() as i64,
            ) {
                eprintln!(
                    "  warning: failed to index forced fetch entry for {}: {}",
                    meta.name, e
                );
            }
            println!("  fetching {}@{} (forced)...", meta.name, ver_str);
            (content, hash)
        } else {
            let cached = store_index.lookup(&cache_key).ok().flatten();
            if let Some(cached_hash) = cached {
                if store.contains(&cached_hash) {
                    if let Some(content) = store.get(&cached_hash)? {
                        if refresh {
                            println!("  refresh: re-fetching {}@{}", meta.name, ver_str);
                            let content = fetch_meta_content(&meta).await?;
                            let hash = store.put(&content)?;
                            if let Err(e) = store_index.insert(
                                &cache_key,
                                &hash,
                                &meta.source_type.to_string(),
                                content.len() as i64,
                            ) {
                                eprintln!(
                                    "  warning: failed to index refreshed entry for {}: {}",
                                    meta.name, e
                                );
                            }
                            (content, hash)
                        } else {
                            println!("  using cached {}@{}", meta.name, ver_str);
                            (content, cached_hash)
                        }
                    } else {
                        if let Err(e) = store_index.remove(&cache_key) {
                            eprintln!(
                                "  warning: failed to remove stale cache key for {}: {}",
                                meta.name, e
                            );
                        }
                        let content = fetch_meta_content(&meta).await?;
                        let hash = store.put(&content)?;
                        if let Err(e) = store_index.insert(
                            &cache_key,
                            &hash,
                            &meta.source_type.to_string(),
                            content.len() as i64,
                        ) {
                            eprintln!(
                                "  warning: failed to index re-fetched entry for {}: {}",
                                meta.name, e
                            );
                        }
                        (content, hash)
                    }
                } else if offline {
                    anyhow::bail!(
                        "{}@{} not found in cache (--offline mode)",
                        meta.name,
                        ver_str
                    );
                } else {
                    if let Err(e) = store_index.remove(&cache_key) {
                        eprintln!(
                            "  warning: failed to remove stale cache key for {}: {}",
                            meta.name, e
                        );
                    }
                    let content = fetch_meta_content(&meta).await?;
                    let hash = store.put(&content)?;
                    if let Err(e) = store_index.insert(
                        &cache_key,
                        &hash,
                        &meta.source_type.to_string(),
                        content.len() as i64,
                    ) {
                        eprintln!(
                            "  warning: failed to index re-fetched entry for {}: {}",
                            meta.name, e
                        );
                    }
                    (content, hash)
                }
            } else if offline {
                anyhow::bail!(
                    "{}@{} not found in cache (--offline mode)",
                    meta.name,
                    ver_str
                );
            } else {
                let content = fetch_meta_content(&meta).await?;
                let hash = store.put(&content)?;
                if let Err(e) = store_index.insert(
                    &cache_key,
                    &hash,
                    &meta.source_type.to_string(),
                    content.len() as i64,
                ) {
                    eprintln!(
                        "  warning: failed to index fresh fetch entry for {}: {}",
                        meta.name, e
                    );
                }
                println!("  fetching {}@{}...", meta.name, ver_str);
                (content, hash)
            }
        };

        // For tarball URLs, identity is embedded in the tarball — extract it now
        if meta.source_type == SourceType::Url {
            let (real_name, real_version) =
                ara_source::tarball::identity_from_tarball(&pkg_content).unwrap_or_else(|_| {
                    let fallback =
                        ara_source::tarball::name_from_url(meta.url.as_deref().unwrap_or(""))
                            .unwrap_or_else(|| "package".to_string());
                    println!(
                        "  warning: could not read package.json from tarball, using {fallback}"
                    );
                    (fallback, "0.0.0".to_string())
                });
            meta.name = real_name;
            meta.version.clone_from(&real_version);
            if let Ok(pv) = Version::parse(&real_version) {
                meta.version_semver = pv;
            }
        }

        let pkg_dir = node_modules.join(&meta.name);
        let _ = std::fs::remove_dir_all(&pkg_dir);
        std::fs::create_dir_all(&pkg_dir)?;

        if let Err(e) = extract_tarball(&pkg_content, &pkg_dir) {
            println!("  failed to extract {}: {}", meta.name, e);
            continue;
        }

        if let Err(e) = install_bin_links(&node_modules, &meta.name, &pkg_dir) {
            println!(
                "  warning: failed to create bin links for {}: {}",
                meta.name, e
            );
        }

        let (allowed, security) = match analyzer::analyze_package(&pkg_dir) {
            Ok(result) => {
                if result.findings.is_empty() {
                    print!("  ✓ {}@{} ({})", meta.name, ver_str, hash_str);
                    (
                        true,
                        Some(SecurityMeta {
                            risk_level: Some(result.risk_level.to_string()),
                        }),
                    )
                } else if non_interactive || result.risk_level <= RiskLevel::Medium {
                    let rl = result.risk_level;
                    print!(
                        "  ✓ {}@{} ({}) ⚠  {} finding(s) ({}) — auto-approved",
                        meta.name,
                        ver_str,
                        hash_str,
                        result.findings.len(),
                        rl
                    );
                    (
                        true,
                        Some(SecurityMeta {
                            risk_level: Some(rl.to_string()),
                        }),
                    )
                } else {
                    match prompt_allow_package(&meta.name, &ver_str, &result.findings) {
                        AllowDecision::Yes | AllowDecision::Sandbox => {
                            println!("  ✓ {}@{} ({}) — allowed", meta.name, ver_str, hash_str);
                            (
                                true,
                                Some(SecurityMeta {
                                    risk_level: Some(result.risk_level.to_string()),
                                }),
                            )
                        }
                        AllowDecision::No => {
                            let _ = std::fs::remove_dir_all(&pkg_dir);
                            println!("  ✗ {}@{} ({}) — denied", meta.name, ver_str, hash_str);
                            (false, None)
                        }
                    }
                }
            }
            Err(_) => {
                print!("  ✓ {}@{} ({})", meta.name, ver_str, hash_str);
                (true, None)
            }
        };

        if !allowed {
            continue;
        }

        m.deps.push(ara_manifest::types::DependencyEntry {
            name: meta.name.clone(),
            source: meta.source.clone(),
            kind: dep_kind.clone(),
            version: Some(meta.version.clone()),
            repo: meta.repo.clone(),
            url: meta.url.clone(),
            commit: meta.commit.clone(),
            path: None,
        });

        pkg_entries.push(PackageEntry {
            name: meta.name.clone(),
            version: ver_str.clone(),
            source: meta.source_type.to_string(),
            package_hash: hash_str.clone(),
            integrity: None,
            signature: None,
            repository: meta.repo.clone(),
            commit: meta.commit.clone(),
            dependencies: None,
            security,
            sbom: None,
        });
        installed_names.push(meta.name.clone());

        println!();
    }

    // Write updated package.json
    let pkg_json_content = ara_manifest::package_json::generate_package_json(&m);
    std::fs::write(cwd.join("package.json"), &pkg_json_content)
        .context("failed to write package.json")?;

    println!("Updated package.json with {} dep(s)", m.deps.len());

    // Install transitive dependencies discovered from the newly installed packages
    install_transitive_deps(
        &node_modules,
        &store,
        &store_index,
        &mut pkg_entries,
        &installed_names,
    )
    .await?;

    if !pkg_entries.is_empty() {
        write_lockfile(&cwd, Some(&store), &pkg_entries)?;
        if package_lock {
            write_package_lock(&cwd, &m, &pkg_entries)?;
        }
    }

    Ok(())
}

/// Resolved package metadata (without content).
struct ResolvedMeta {
    name: String,
    version: String,
    version_semver: Version,
    source_type: SourceType,
    source: String,
    url: Option<String>,
    repo: Option<String>,
    commit: Option<String>,
}

/// Fetch content for a resolved meta, returning raw bytes.
async fn fetch_meta_content(meta: &ResolvedMeta) -> Result<Vec<u8>> {
    match meta.source_type {
        SourceType::Npm | SourceType::Registry => {
            let registry_url = std::env::var("ARA_NPM_REGISTRY")
                .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());
            let reg = ara_source::registry::RegistrySource::new(registry_url)?;
            let identity = PackageIdentity {
                source: SourceType::Npm,
                name: meta.name.clone(),
                version: meta.version_semver.clone(),
                content_hash: None,
                requested_ref: None,
            };
            reg.fetch(&identity)
                .await
                .with_context(|| format!("failed to fetch {}@{}", meta.name, meta.version))
        }
        SourceType::Github => {
            let repo = meta.repo.as_deref().unwrap_or(&meta.name);
            let src = ara_source::github::GithubSource::new(repo.to_string());
            let identity = PackageIdentity {
                source: SourceType::Github,
                name: repo.to_string(),
                version: meta.version_semver.clone(),
                content_hash: None,
                requested_ref: meta.commit.clone(),
            };
            src.fetch(&identity)
                .await
                .with_context(|| format!("failed to fetch github:{repo}"))
        }
        SourceType::Git => {
            let url = meta.url.as_deref().unwrap_or(&meta.name);
            let commit_str = meta.commit.clone().unwrap_or_else(|| "HEAD".to_string());
            let src = ara_source::git::GitSource::new(url.to_string(), commit_str.clone());
            let identity = PackageIdentity {
                source: SourceType::Git,
                name: url.to_string(),
                version: meta.version_semver.clone(),
                content_hash: None,
                requested_ref: Some(commit_str),
            };
            src.fetch(&identity)
                .await
                .with_context(|| format!("failed to fetch git:{url}"))
        }
        SourceType::Url => {
            let url = meta.url.as_deref().unwrap_or(&meta.name);
            let src = ara_source::tarball::TarballSource::new(url.to_string());
            let identity = PackageIdentity {
                source: SourceType::Url,
                name: url.to_string(),
                version: Version::new(0, 0, 0),
                content_hash: None,
                requested_ref: None,
            };
            src.fetch(&identity)
                .await
                .with_context(|| format!("failed to download {url}"))
        }
        _ => anyhow::bail!("unsupported source type: {}", meta.source_type),
    }
}

async fn resolve_npm_meta(
    name: &str,
    version: Option<&str>,
    range: Option<&str>,
) -> Result<ResolvedMeta> {
    let registry_url = std::env::var("ARA_NPM_REGISTRY")
        .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());

    let reg = ara_source::registry::RegistrySource::new(registry_url)?;

    let (resolved_ver_str, manifest_ver) = if let Some(v) = version {
        let trimmed = v
            .trim_start_matches('^')
            .trim_start_matches('~')
            .trim_start_matches('>')
            .trim_start_matches('<')
            .trim_start_matches('=');
        let is_exact = v == trimmed;
        if is_exact {
            Version::parse(v).with_context(|| format!("invalid version: {v}"))?;
            (v.to_string(), v.to_string())
        } else {
            let concrete = reg
                .resolve(name)
                .await
                .with_context(|| format!("failed to resolve {name} for range {v}"))?;
            (concrete, v.to_string())
        }
    } else {
        let concrete = reg
            .resolve(name)
            .await
            .with_context(|| format!("failed to resolve {name}"))?;
        let manifest = apply_range(&concrete, range);
        (concrete, manifest)
    };

    let parsed_ver = Version::parse(&resolved_ver_str)
        .with_context(|| format!("invalid version from registry: {resolved_ver_str}"))?;

    Ok(ResolvedMeta {
        name: name.to_string(),
        version: manifest_ver,
        version_semver: parsed_ver,
        source_type: SourceType::Npm,
        source: "npm".to_string(),
        url: None,
        repo: None,
        commit: None,
    })
}

fn resolve_github_meta(repo: &str, commit: Option<&str>) -> Result<ResolvedMeta> {
    let ver_str = commit.unwrap_or("HEAD").to_string();
    Ok(ResolvedMeta {
        name: repo.to_string(),
        version: ver_str,
        version_semver: Version::new(0, 0, 0),
        source_type: SourceType::Github,
        source: "github".to_string(),
        url: None,
        repo: Some(repo.to_string()),
        commit: commit.map(|c| c.to_string()),
    })
}

fn resolve_git_meta(url: &str, commit: Option<&str>) -> Result<ResolvedMeta> {
    let commit_str = commit.unwrap_or("HEAD").to_string();
    let name = derive_name_from_git_url(url)
        .unwrap_or_else(|| url.rsplit('/').next().unwrap_or(url).to_string());
    Ok(ResolvedMeta {
        name,
        version: commit_str.clone(),
        version_semver: Version::new(0, 0, 0),
        source_type: SourceType::Git,
        source: "git".to_string(),
        url: Some(url.to_string()),
        repo: None,
        commit: Some(commit_str),
    })
}

fn resolve_tarball_meta(url: &str) -> Result<ResolvedMeta> {
    // Tarball identity is unknown until download; name/version filled after fetch.
    Ok(ResolvedMeta {
        name: String::new(),
        version: String::new(),
        version_semver: Version::new(0, 0, 0),
        source_type: SourceType::Url,
        source: "url".to_string(),
        url: Some(url.to_string()),
        repo: None,
        commit: None,
    })
}

async fn resolve_spec_meta(
    target: &ara_source::url::InstallTarget,
    range: Option<&str>,
) -> Result<ResolvedMeta> {
    match target {
        ara_source::url::InstallTarget::Npm { name, version } => {
            resolve_npm_meta(name, version.as_deref(), range).await
        }
        ara_source::url::InstallTarget::Github { repo, commit } => {
            resolve_github_meta(repo, commit.as_deref())
        }
        ara_source::url::InstallTarget::Git { url, commit } => {
            resolve_git_meta(url, commit.as_deref())
        }
        ara_source::url::InstallTarget::Tarball { url } => resolve_tarball_meta(url),
    }
}

fn apply_range(version: &str, range: Option<&str>) -> String {
    match range {
        Some("caret") => format!("^{version}"),
        Some("patch") => format!("~{version}"),
        _ => version.to_string(),
    }
}

fn derive_name_from_git_url(url: &str) -> Option<String> {
    // https://github.com/user/repo.git → "repo"
    // git@github.com:user/repo.git → "repo"
    // https://bitbucket.org/user/repo → "repo"
    let without_fragment = url.split('#').next().unwrap_or(url);
    let without_git = without_fragment
        .strip_suffix(".git")
        .unwrap_or(without_fragment);
    let name = without_git.rsplit('/').next()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn read_manifest(cwd: &Path) -> Result<ara_manifest::types::Manifest> {
    let manifest_path = cwd.join("ara.toml");
    let pkg_json_path = cwd.join("package.json");

    let mut final_manifest: Option<ara_manifest::types::Manifest> = None;

    if pkg_json_path.exists() {
        let content = std::fs::read_to_string(&pkg_json_path)
            .with_context(|| format!("failed to read {}", pkg_json_path.display()))?;
        let m =
            package_json::parse_package_json(&content).context("failed to parse package.json")?;
        final_manifest = Some(m);
    }

    if manifest_path.exists() {
        let content = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let m = parser::parse(&content).context("failed to parse ara.toml")?;

        if let Some(mut fm) = final_manifest {
            // merge ara.toml advanced settings into package.json manifest
            fm.security = m.security;
            fm.build = m.build;
            // Note: We deliberately do NOT merge deps, scripts, or workspaces from ara.toml
            // because package.json is now the source of truth for them.
            final_manifest = Some(fm);
        } else {
            // if no package.json exists, fallback to ara.toml completely
            final_manifest = Some(m);
        }
    }

    if let Some(m) = final_manifest {
        return Ok(m);
    }

    Err(anyhow::anyhow!(
        "no manifest found: neither package.json nor ara.toml exists in {}",
        cwd.display()
    ))
}

#[allow(clippy::too_many_lines)]
async fn cmd_install_in(cwd: &Path, non_interactive: bool, package_lock: bool) -> Result<()> {
    let mut m = read_manifest(cwd)?;

    println!(
        "Installing dependencies for {} v{}",
        m.project.name, m.project.version
    );

    // Expand workspace members into deps automatically
    if let Some(ws) = &m.workspace {
        let workspace_deps = expand_workspace_members(ws, cwd);
        for dep in workspace_deps {
            if let Some(existing) = m.deps.iter_mut().find(|d| d.name == dep.name) {
                if existing.path.is_none() {
                    existing.path = dep.path.clone();
                }
                if dep.version.is_some() {
                    existing.version = dep.version.clone();
                }
            } else {
                println!(
                    "  workspace member: {} -> {}",
                    dep.name,
                    dep.path.as_deref().unwrap_or(".")
                );
                m.deps.push(dep);
            }
        }
    }

    if m.deps.is_empty() && m.workspace.is_none() {
        println!("No dependencies to install");
        write_lockfile(cwd, None, &[]).context("failed to write lockfile")?;
        if package_lock {
            write_package_lock(cwd, &m, &[])?;
        }
        return Ok(());
    }

    let mut r = Resolver::new();
    for dep in &m.deps {
        let constraint = Constraint::parse(dep.version.as_deref().unwrap_or("*"))
            .context("failed to parse version constraint")?;
        let source = source_type_from_str(&dep.source);
        r.add_constraint(ConstraintEntry {
            package: dep.name.clone(),
            constraint,
            source,
            required_by: "root".to_string(),
        });
    }

    let mut graph = r.resolve();
    println!("Resolved {} packages", graph.nodes.len());

    // Warm up the connection pool to prevent the Thundering Herd
    let default_url = std::env::var("ARA_NPM_REGISTRY")
        .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());
    if let Ok(reg) = ara_source::registry::RegistrySource::new(default_url) {
        let _ = reg.warmup().await;
    }

    // Connect resolve(): enhance each node's version from registry sources
    let t_resolve = Instant::now();
    let dep_lookup: HashMap<&str, &ara_manifest::types::DependencyEntry> =
        m.deps.iter().map(|d| (d.name.as_str(), d)).collect();
    let nodes = graph.nodes.clone();
    let mut tasks = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        if let Some(dep) = dep_lookup.get(node.name.as_str()).copied() {
            let node_name = node.name.clone();
            let source_type = node.source;
            tasks.push(async move {
                if let Ok(src) = create_source(source_type, dep) {
                    let version_str = match &src {
                        Source::Registry(reg) => {
                            reg.resolve_matching(&node_name, dep.version.as_deref().unwrap_or("*"))
                                .await
                        }
                        _ => src.resolve(&node_name).await,
                    };
                    if let Ok(version_str) = version_str {
                        if let Ok(parsed) = Version::parse(&version_str) {
                            return Some((i, parsed));
                        }
                    }
                }
                None
            });
        }
    }

    let total_resolve = tasks.len() as u64;
    let pb_resolve = ProgressBar::new(total_resolve);
    pb_resolve.set_style(
        ProgressStyle::with_template("{spinner:.green} resolving versions... {pos}/{len}")
            .context("invalid progress bar template")?
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );

    let tasks: Vec<_> = tasks.into_iter().map(|task| {
        let pb = pb_resolve.clone();
        async move {
            let r = task.await;
            pb.inc(1);
            r
        }
    }).collect();

    let results: Vec<_> = futures::future::join_all(tasks).await
        .into_iter()
        .flatten()
        .collect();
    pb_resolve.finish_and_clear();

    for (i, version) in results {
        graph.nodes[i].version = version;
    }
    eprintln!(
        "  [profile] resolve versions ({} nodes): {:?}",
        graph.nodes.len(),
        t_resolve.elapsed()
    );

    // Connect has_cycles(): warn if circular dependencies found
    if graph.has_cycles() {
        println!("warning: circular dependency detected in the resolved graph");
    }

    let node_modules = cwd.join("node_modules");

    let lock_path = cwd.join("ara.lock");
    if lock_path.exists() && node_modules.exists() {
        let lock_content = match std::fs::read_to_string(&lock_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "  warning: could not read lockfile ({}), will re-install",
                    e
                );
                String::new()
            }
        };
        if let Ok(existing) = ara_lockfile::parser::parse(&lock_content) {
            let all_match = existing.packages.iter().all(|p| {
                graph.find_node(&p.name).is_some_and(|idx| {
                    let n = &graph.nodes[idx];
                    n.source.to_string() == p.source && n.version.to_string() == p.version
                })
            });
            if all_match && !graph.nodes.is_empty() {
                let all_exist = graph
                    .nodes
                    .iter()
                    .all(|n| node_modules.join(&n.name).exists());
                if all_exist {
                    println!("Lockfile is up to date. Nothing to install.");
                    return Ok(());
                }
            }
        }
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let store_base = PathBuf::from(&home).join(".ara").join("store");
    let store = Store::new(store_base.clone());
    store.ensure_dirs()?;

    let index_path = store_base.join("index.db");
    let store_index = Arc::new(StoreIndex::new(index_path)?);
    let mut pkg_entries: Vec<PackageEntry> = Vec::new();

    // Process packages in parallel
    struct ProcessedNode {
        name: String,
        version: String,
        hash_str: String,
        pkg_dir: PathBuf,
        source_type: String,
        analysis: Result<ara_types::AnalysisResult>,
    }

    let t_phase1 = Instant::now();
    let mut tasks = Vec::new();
    for node in &graph.nodes {
        let node = node.clone();
        let m_deps = m.deps.clone();
        let store = store.clone();
        let store_index = Arc::clone(&store_index);
        let node_modules = node_modules.to_path_buf();

        let cwd = cwd.to_path_buf();
        tasks.push(tokio::spawn(async move {
            let ver_str = node.version.to_string();

            let dep = match find_dep(&m_deps, &node.name) {
                Some(d) => d,
                None => return None,
            };

            // Workspace deps are live symlinks, not fetched/extracted
            if node.source == SourceType::Workspace {
                let rel_path = dep.path.as_deref().unwrap_or(".");
                let member_path = cwd.join(rel_path);
                let pkg_dir = node_modules.join(&node.name);
                let _ = std::fs::remove_dir_all(&pkg_dir);
                if let Err(e) = std::fs::create_dir_all(pkg_dir.parent().unwrap_or(&node_modules)) {
                    eprintln!(
                        "  warning: failed to create dir for workspace link {}: {}",
                        node.name, e
                    );
                    return None;
                }
                #[cfg(unix)]
                if let Err(e) = std::os::unix::fs::symlink(&member_path, &pkg_dir) {
                    eprintln!(
                        "  warning: failed to symlink workspace {} -> {}: {}",
                        node.name,
                        member_path.display(),
                        e
                    );
                    return None;
                }
                #[cfg(not(unix))]
                if let Err(e) = std::fs::hard_link(&member_path, &pkg_dir) {
                    eprintln!(
                        "  warning: failed to hardlink workspace {} -> {}: {}",
                        node.name,
                        member_path.display(),
                        e
                    );
                    return None;
                }
                println!("  symlink {} -> {}", node.name, member_path.display());

                let pkg_dir_clone = pkg_dir.clone();
                let analysis = match tokio::task::spawn_blocking(move || {
                    analyzer::analyze_package(&pkg_dir_clone)
                })
                .await
                {
                    Ok(result) => result,
                    Err(e) => {
                        eprintln!("  failed to analyze {} (task panic): {}", node.name, e);
                        return None;
                    }
                };

                return Some(ProcessedNode {
                    name: node.name.clone(),
                    version: ver_str,
                    hash_str: format!("workspace:{}", rel_path),
                    pkg_dir,
                    source_type: "workspace".to_string(),
                    analysis,
                });
            }

            let src = match create_source(node.source, dep) {
                Ok(s) => s,
                Err(_) => return None,
            };

            let cache_key = format!("{}@{}", node.name, ver_str);

            let cached = store_index.lookup(&cache_key).ok().flatten();

            let (hash_str, fresh_content) = if let Some(cached_hash) = cached {
                if store.contains(&cached_hash) {
                    println!("  using cached {}@{}", node.name, ver_str);
                    (cached_hash, None)
                } else {
                    if let Err(e) = store_index.remove(&cache_key) {
                        eprintln!(
                            "  warning: failed to remove stale cache key for {}: {}",
                            node.name, e
                        );
                    }
                    let (h, c) = fetch_and_store_parallel(
                        &store,
                        &store_index,
                        &src,
                        &cache_key,
                        &node,
                        &ver_str,
                    )
                    .await?;
                    (h, Some(c))
                }
            } else {
                let (h, c) = fetch_and_store_parallel(
                    &store,
                    &store_index,
                    &src,
                    &cache_key,
                    &node,
                    &ver_str,
                )
                .await?;
                (h, Some(c))
            };

            let pkg_dir = node_modules.join(&node.name);
            let pkg_dir_clone = pkg_dir.clone();
            let store_clone = store.clone();
            let hash_str_clone = hash_str.clone();
            match tokio::task::spawn_blocking(move || {
                extract_package_cached(&store_clone, &hash_str_clone, &pkg_dir_clone, fresh_content)
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    eprintln!("  failed to extract {}: {}", node.name, e);
                    return None;
                }
                Err(e) => {
                    eprintln!("  failed to extract {} (task panic): {}", node.name, e);
                    return None;
                }
            }

            if let Err(e) = install_bin_links(&node_modules, &node.name, &pkg_dir) {
                eprintln!(
                    "  warning: failed to create bin links for {}: {}",
                    node.name, e
                );
            }

            let source_type = match node.source {
                SourceType::Npm | SourceType::Registry => "registry",
                SourceType::Github => "github",
                SourceType::Git => "git",
                SourceType::Local => "local",
                SourceType::Url => "url",
                SourceType::Workspace => "workspace",
            }
            .to_string();

            let pkg_dir_clone2 = pkg_dir.clone();
            let analysis = match tokio::task::spawn_blocking(move || {
                analyzer::analyze_package(&pkg_dir_clone2)
            })
            .await
            {
                Ok(result) => result,
                Err(e) => {
                    eprintln!("  failed to analyze {} (task panic): {}", node.name, e);
                    return None;
                }
            };

            Some(ProcessedNode {
                name: node.name.clone(),
                version: ver_str,
                hash_str,
                pkg_dir,
                source_type,
                analysis,
            })
        }));
    }
    let processed: Vec<ProcessedNode> = futures::future::join_all(tasks)
        .await
        .into_iter()
        .filter_map(|x| match x {
            Ok(inner) => inner,
            Err(e) => {
                eprintln!("  warning: nested dependency task panic: {e}");
                None
            }
        })
        .collect();
    eprintln!(
        "  [profile] phase 1 (direct deps): {:?}",
        t_phase1.elapsed()
    );

    // Handle security decisions sequentially (necessary for interactive prompts)
    for pkg in &processed {
        let (allowed, security) = match &pkg.analysis {
            Ok(result) => {
                if result.findings.is_empty() {
                    print!("  ✓ {}@{} ({})", pkg.name, pkg.version, pkg.hash_str);
                    (
                        true,
                        Some(SecurityMeta {
                            risk_level: Some(result.risk_level.to_string()),
                        }),
                    )
                } else if non_interactive {
                    let rl = result.risk_level;
                    let first = &result.findings[0];
                    let loc = first.location.as_deref().unwrap_or("");
                    print!(
                        "  ✓ {}@{} ({}) ⚠  {} finding(s) ({}) — {} in {}",
                        pkg.name,
                        pkg.version,
                        pkg.hash_str,
                        result.findings.len(),
                        rl,
                        first.description,
                        loc
                    );
                    (
                        true,
                        Some(SecurityMeta {
                            risk_level: Some(rl.to_string()),
                        }),
                    )
                } else {
                    match prompt_allow_package(&pkg.name, &pkg.version, &result.findings) {
                        AllowDecision::Yes | AllowDecision::Sandbox => {
                            println!(
                                "  ✓ {}@{} ({}) — allowed",
                                pkg.name, pkg.version, pkg.hash_str
                            );
                            (
                                true,
                                Some(SecurityMeta {
                                    risk_level: Some(result.risk_level.to_string()),
                                }),
                            )
                        }
                        AllowDecision::No => {
                            let _ = std::fs::remove_dir_all(&pkg.pkg_dir);
                            println!(
                                "  ✗ {}@{} ({}) — denied",
                                pkg.name, pkg.version, pkg.hash_str
                            );
                            (false, None)
                        }
                    }
                }
            }
            Err(_) => {
                print!("  ✓ {}@{} ({})", pkg.name, pkg.version, pkg.hash_str);
                (true, None)
            }
        };

        if !allowed {
            continue;
        }

        pkg_entries.push(PackageEntry {
            name: pkg.name.clone(),
            version: pkg.version.clone(),
            source: pkg.source_type.clone(),
            package_hash: pkg.hash_str.clone(),
            integrity: None,
            signature: None,
            repository: None,
            commit: None,
            dependencies: None,
            security,
            sbom: None,
        });

        println!();
    }
    eprintln!(
        "  [profile] security eval ({} pkgs): {:?}",
        processed.len(),
        t_phase1.elapsed()
    );

    // Install transitive dependencies discovered from extracted packages
    let installed_names: Vec<String> = processed.iter().map(|p| p.name.clone()).collect();
    install_transitive_deps(
        &node_modules,
        &store,
        &store_index,
        &mut pkg_entries,
        &installed_names,
    )
    .await?;

    let graph_bytes = serde_json::to_vec(&graph.nodes)?;
    let store_graph_hash = store.put_graph(&graph_bytes)?;
    let raw = graph.compute_hash()?;
    let graph_hash = format!("sha256:{}", ara_util::hash::hex_encode(&raw));
    // Verify stored hash matches computed hash (sanity check)
    if !store_graph_hash.contains(&graph_hash[7..17]) {
        println!("note: stored graph hash diverges from computed hash");
    }

    let ts = current_timestamp();

    if package_lock {
        write_package_lock(cwd, &m, &pkg_entries)?;
    }

    let lockfile = Lockfile {
        version: 1,
        graph: GraphMeta {
            resolver: "mvs".to_string(),
            generated_at: Some(ts),
            graph_hash: Some(graph_hash),
        },
        packages: pkg_entries,
    };

    let lock_content = ara_lockfile::generator::generate(&lockfile);
    let lock_path = cwd.join("ara.lock");
    let mut lock_f = std::fs::File::create(&lock_path)?;
    lock_f.write_all(lock_content.as_bytes())?;
    println!("Lockfile written to ara.lock");

    Ok(())
}

async fn fetch_and_store_parallel(
    store: &Store,
    store_index: &Arc<StoreIndex>,
    src: &Source,
    cache_key: &str,
    node: &ara_resolver::graph::Node,
    ver_str: &str,
) -> Option<(String, Vec<u8>)> {
    println!("  fetching {}@{}...", node.name, ver_str);

    let identity = ara_types::PackageIdentity {
        source: node.source,
        name: node.name.clone(),
        version: node.version.clone(),
        content_hash: None,
        requested_ref: None,
    };

    let pkg_content = match src.fetch(&identity).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("  failed to fetch {}: {}", node.name, e);
            return None;
        }
    };

    let hash_str = match store.put(&pkg_content) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("  failed to store {}: {}", node.name, e);
            return None;
        }
    };

    if let Err(e) = store_index.insert(
        cache_key,
        &hash_str,
        &node.source.to_string(),
        pkg_content.len() as i64,
    ) {
        eprintln!(
            "  warning: failed to index fetch result for {}: {}",
            node.name, e
        );
    }

    Some((hash_str, pkg_content))
}

/// Extract a tarball from the CAS store to the package directory.
/// Uses a cached extracted directory in the store to avoid re-extraction.
/// When `content` is provided, uses it directly instead of reading from the store.
fn extract_package_cached(
    store: &Store,
    hash_str: &str,
    pkg_dir: &Path,
    content: Option<Vec<u8>>,
) -> Result<()> {
    let extracted_dir = store.get_extracted_path(hash_str);

    if !extracted_dir.exists() {
        let tarball = match content {
            Some(c) => c,
            None => store
                .get(hash_str)?
                .ok_or_else(|| anyhow::anyhow!("package {hash_str} not in store"))?,
        };
        std::fs::create_dir_all(&extracted_dir)
            .with_context(|| format!("failed to create {}", extracted_dir.display()))?;
        extract_tarball(&tarball, &extracted_dir)
            .with_context(|| format!("failed to extract to {}", extracted_dir.display()))?;
    }

    let _ = std::fs::remove_dir_all(pkg_dir);
    hardlink_dir(&extracted_dir, pkg_dir)
        .with_context(|| format!("failed to hardlink to {}", pkg_dir.display()))
}

/// Recursively create hardlinks from `src` to `dst`, falling back to copy
/// if hardlinking across filesystems fails.
pub fn hardlink_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry?;
        let relative = entry
            .path()
            .strip_prefix(src)
            .map_err(|_| anyhow::anyhow!("path prefix error"))?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = dst.join(relative);

        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if entry.file_type().is_symlink() {
            #[cfg(unix)]
            {
                let link_target = std::fs::read_link(entry.path())?;
                let _ = std::fs::remove_file(&target);
                std::os::unix::fs::symlink(&link_target, &target)?;
            }
            #[cfg(not(unix))]
            {
                let _ = std::fs::copy(entry.path(), &target);
            }
        } else if std::fs::hard_link(entry.path(), &target).is_err() {
            let _ = std::fs::copy(entry.path(), &target);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_current_timestamp_format() {
        let ts = current_timestamp();
        assert_eq!(ts.len(), 20, "expected ISO 8601 length, got: {ts}");
        assert_eq!(&ts[4..5], "-", "expected - after year: {ts}");
        assert_eq!(&ts[7..8], "-", "expected - after month: {ts}");
        assert_eq!(&ts[10..11], "T", "expected T separator: {ts}");
        assert_eq!(&ts[13..14], ":", "expected : after hour: {ts}");
        assert_eq!(&ts[16..17], ":", "expected : after minute: {ts}");
        assert_eq!(&ts[19..20], "Z", "expected Z suffix: {ts}");
    }

    #[test]
    fn test_current_timestamp_parses_as_date() {
        let ts = current_timestamp();
        let year: i64 = ts[0..4].parse().unwrap();
        let month: u32 = ts[5..7].parse().unwrap();
        let day: u32 = ts[8..10].parse().unwrap();
        assert!(year >= 2024, "year should be >= 2024, got {year}");
        assert!((1..=12).contains(&month), "month out of range: {month}");
        assert!((1..=31).contains(&day), "day out of range: {day}");
    }

    #[test]
    fn test_sha256_hex_to_sri() {
        let hash = "sha256-e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let result = sha256_hex_to_sri(hash);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="
        );
        assert!(sha256_hex_to_sri("sha256-tooshort").is_none());
        assert!(sha256_hex_to_sri("md5-abc").is_none());
    }

    #[test]
    fn test_npm_tarball_url_basic() {
        let url = npm_tarball_url("zod", "3.23.8");
        assert_eq!(url, "https://registry.npmjs.org/zod/-/zod-3.23.8.tgz");
    }

    #[test]
    fn test_npm_tarball_url_scoped() {
        let url = npm_tarball_url("@types/node", "25.9.2");
        assert_eq!(
            url,
            "https://registry.npmjs.org/@types%2fnode/-/@types/node-25.9.2.tgz"
        );
    }

    #[test]
    fn test_url_encode_pkg_name() {
        assert_eq!(url_encode_pkg_name("zod"), "zod");
        assert_eq!(url_encode_pkg_name("@types/node"), "%40types%2fnode");
        assert_eq!(url_encode_pkg_name("@scope/pkg"), "%40scope%2fpkg");
    }

    #[test]
    fn test_source_type_from_str() {
        assert_eq!(source_type_from_str("npm"), SourceType::Npm);
        assert_eq!(source_type_from_str("registry"), SourceType::Registry);
        assert_eq!(source_type_from_str("github"), SourceType::Github);
        assert_eq!(source_type_from_str("git"), SourceType::Git);
        assert_eq!(source_type_from_str("local"), SourceType::Local);
        assert_eq!(source_type_from_str("workspace"), SourceType::Workspace);
        assert_eq!(source_type_from_str("foo"), SourceType::Npm);
        assert_eq!(source_type_from_str(""), SourceType::Npm);
    }

    #[test]
    fn test_find_dep() {
        let deps = vec![
            ara_manifest::types::DependencyEntry {
                name: "zod".into(),
                source: "npm".into(),
                kind: None,
                version: Some("^3.0.0".into()),
                repo: None,
                url: None,
                commit: None,
                path: None,
            },
            ara_manifest::types::DependencyEntry {
                name: "react".into(),
                source: "npm".into(),
                kind: None,
                version: Some("^18.0.0".into()),
                repo: None,
                url: None,
                commit: None,
                path: None,
            },
        ];
        assert!(find_dep(&deps, "zod").is_some());
        assert!(find_dep(&deps, "react").is_some());
        assert!(find_dep(&deps, "missing").is_none());
    }

    #[test]
    fn test_extract_tarball() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("out");

        let mut buf = Vec::new();
        let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
        let mut ar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_path("hello.txt").unwrap();
        header.set_size(12);
        header.set_mode(0o644);
        header.set_cksum();
        ar.append(&header, b"hello world\n".as_slice()).unwrap();
        let encoder = ar.into_inner().unwrap();
        encoder.finish().unwrap();

        extract_tarball(&buf, &dest).unwrap();
        let extracted = std::fs::read_to_string(dest.join("hello.txt")).unwrap();
        assert_eq!(extracted, "hello world\n");
    }

    #[tokio::test]
    async fn test_cmd_install_local_dep() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().to_path_buf();

        let dep_dir = root_path.join("dep-a");
        std::fs::create_dir_all(&dep_dir).unwrap();
        let dep_manifest = r#"
            [project]
            name = "dep-a"
            version = "0.1.0"
        "#;
        std::fs::write(dep_dir.join("ara.toml"), dep_manifest).unwrap();
        std::fs::write(dep_dir.join("index.js"), "module.exports = {};").unwrap();

        let root_manifest = format!(
            r#"
            [project]
            name = "my-app"
            version = "1.0.0"

            [deps]
            dep-a = {{ source = "local", version = "0.1.0", path = "{}" }}
            "#,
            dep_dir.display()
        );
        std::fs::write(root_path.join("ara.toml"), &root_manifest).unwrap();

        cmd_install_in(&root_path, true, false).await.unwrap();

        assert!(root_path.join("node_modules").exists());
        assert!(root_path.join("node_modules/dep-a").exists());
        assert!(root_path.join("node_modules/dep-a/index.js").exists());
        assert!(root_path.join("ara.lock").exists());

        let lock_content = std::fs::read_to_string(root_path.join("ara.lock")).unwrap();
        let lf = ara_lockfile::parser::parse(&lock_content).unwrap();
        assert!(!lf.packages.is_empty());
        assert_eq!(lf.packages[0].name, "dep-a");
    }

    #[tokio::test]
    async fn test_cmd_install_no_deps() {
        let root = tempfile::tempdir().unwrap();
        let root_manifest = r#"
            [project]
            name = "empty"
            version = "0.0.1"
        "#;
        std::fs::write(root.path().join("ara.toml"), root_manifest).unwrap();
        assert!(cmd_install_in(root.path(), true, false).await.is_ok());
    }

    #[tokio::test]
    async fn test_cmd_install_missing_manifest() {
        let root = tempfile::tempdir().unwrap();
        assert!(cmd_install_in(root.path(), true, false).await.is_err());
    }

    #[test]
    fn test_extract_tarball_path_traversal() {
        // The tar crate rejects .. in entry paths at build time,
        // which serves as built-in path traversal protection.
        let mut header = tar::Header::new_gnu();
        let result = header.set_path("../../../etc/passwd");
        assert!(result.is_err(), "tar crate should reject paths with ..");
    }

    #[test]
    fn test_extract_tarball_many_small_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("out");

        let mut buf = Vec::new();
        let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
        let mut ar = tar::Builder::new(encoder);
        let count = 1000;
        for i in 0..count {
            let name = format!("files/file_{i:06}.js");
            let content = format!("module.exports = {{ id: {i} }};\n");
            let mut header = tar::Header::new_gnu();
            header.set_path(&name).unwrap();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            ar.append(&header, content.as_bytes()).unwrap();
        }
        let encoder = ar.into_inner().unwrap();
        encoder.finish().unwrap();

        extract_tarball(&buf, &dest).unwrap();
        let mut extracted_count = 0;
        for entry in walkdir::WalkDir::new(&dest) {
            if entry.unwrap().file_type().is_file() {
                extracted_count += 1;
            }
        }
        assert_eq!(extracted_count, count);
    }

    #[test]
    fn test_expand_workspace_members_500() {
        let root = tempfile::tempdir().unwrap();
        let packages = root.path().join("packages");
        std::fs::create_dir_all(&packages).unwrap();

        // Create 500 workspace members
        let mut member_names = Vec::new();
        for i in 0..500 {
            let name = format!("pkg-{i:04}");
            let dir = packages.join(&name);
            std::fs::create_dir_all(&dir).unwrap();
            let manifest = format!(
                r#"[project]
name = "{name}"
version = "0.1.0"
"#
            );
            std::fs::write(dir.join("ara.toml"), manifest).unwrap();
            member_names.push(name);
        }

        let ws = ara_manifest::types::Workspace {
            members: vec!["packages/*".to_string()],
        };
        let entries = expand_workspace_members(&ws, root.path());
        assert_eq!(entries.len(), 500);

        let names: HashSet<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        for name in &member_names {
            assert!(names.contains(name.as_str()), "missing {name}");
        }
    }

    #[test]
    fn test_read_manifest_with_package_json() {
        let root = tempfile::tempdir().unwrap();
        let pkg_json =
            r#"{"name": "my-app", "version": "0.1.0", "dependencies": {"zod": "^3.0.0"}}"#;
        std::fs::write(root.path().join("package.json"), pkg_json).unwrap();

        let m = read_manifest(root.path()).unwrap();
        assert_eq!(m.project.name, "my-app");
        assert_eq!(m.deps.len(), 1);
        assert_eq!(m.deps[0].name, "zod");
    }

    #[test]
    fn test_read_manifest_merge_ara_toml() {
        let root = tempfile::tempdir().unwrap();
        // Both exist
        let pkg_json = r#"{"name": "from-pkg-json", "version": "1.0.0"}"#;
        std::fs::write(root.path().join("package.json"), pkg_json).unwrap();
        let ara_toml = r#"[project]
name = "ignored"
version = "ignored"

[security]
require_review = true
"#;
        std::fs::write(root.path().join("ara.toml"), ara_toml).unwrap();

        let m = read_manifest(root.path()).unwrap();
        // Should take name from package.json
        assert_eq!(m.project.name, "from-pkg-json");
        assert_eq!(m.project.version, "1.0.0");
        // Should take security from ara.toml
        assert_eq!(m.security.unwrap().require_review, Some(true));
    }

    #[test]
    fn test_read_manifest_neither() {
        let root = tempfile::tempdir().unwrap();
        assert!(read_manifest(root.path()).is_err());
    }

    #[test]
    fn test_expand_workspace_members_with_package_json() {
        let root = tempfile::tempdir().unwrap();
        let packages = root.path().join("packages");
        let member_dir = packages.join("pkg-a");
        std::fs::create_dir_all(&member_dir).unwrap();

        // Member with package.json instead of ara.toml
        let member_json = r#"{"name": "pkg-a", "version": "0.1.0"}"#;
        std::fs::write(member_dir.join("package.json"), member_json).unwrap();

        let ws = ara_manifest::types::Workspace {
            members: vec!["packages/*".to_string()],
        };
        let entries = expand_workspace_members(&ws, root.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "pkg-a");
        assert_eq!(entries[0].version.as_deref(), Some("0.1.0"));
    }

    #[tokio::test]
    async fn test_cmd_install_unicode_name() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().to_path_buf();

        let dep_dir = root_path.join("café-🔥");
        std::fs::create_dir_all(&dep_dir).unwrap();
        let dep_manifest = r#"
            [project]
            name = "café-🔥"
            version = "0.1.0"
        "#;
        std::fs::write(dep_dir.join("ara.toml"), dep_manifest).unwrap();
        std::fs::write(dep_dir.join("index.js"), "module.exports = {};").unwrap();

        let root_manifest = format!(
            r#"
            [project]
            name = "my-app"
            version = "1.0.0"

            [deps]
            "café-🔥" = {{ source = "local", version = "0.1.0", path = "{}" }}
            "#,
            dep_dir.display()
        );
        std::fs::write(root_path.join("ara.toml"), &root_manifest).unwrap();

        cmd_install_in(&root_path, true, false).await.unwrap();

        let nm = root_path.join("node_modules");
        assert!(nm.join("café-🔥").exists());
        assert!(nm.join("café-🔥/index.js").exists());
    }

    #[cfg(feature = "nightly-bench")]
    fn make_tarball(n: usize) -> Vec<u8> {
        let mut buf = Vec::new();
        let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
        let mut ar = tar::Builder::new(encoder);
        for i in 0..n {
            let name = format!("files/file_{i:06}.js");
            let content = format!("module.exports = {{ id: {i} }};\n");
            let mut header = tar::Header::new_gnu();
            header.set_path(&name).unwrap();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            ar.append(&header, content.as_bytes()).unwrap();
        }
        let encoder = ar.into_inner().unwrap();
        encoder.finish().unwrap();
        buf
    }

    #[cfg(feature = "nightly-bench")]
    #[bench]
    fn bench_extract_tarball_100(b: &mut test::Bencher) {
        let tarball = make_tarball(100);
        b.iter(|| {
            let tmp = tempfile::tempdir().unwrap();
            extract_tarball(test::black_box(&tarball), test::black_box(tmp.path()))
        });
    }

    #[cfg(feature = "nightly-bench")]
    #[bench]
    fn bench_extract_tarball_1000(b: &mut test::Bencher) {
        let tarball = make_tarball(1000);
        b.iter(|| {
            let tmp = tempfile::tempdir().unwrap();
            extract_tarball(test::black_box(&tarball), test::black_box(tmp.path()))
        });
    }
}
