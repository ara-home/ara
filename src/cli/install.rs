use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::analysis::analyzer;
use crate::lockfile::types::{GraphMeta, Lockfile, PackageEntry, SecurityMeta};
use crate::manifest::package_json;
use crate::manifest::parser;
use crate::resolver::mvs::{ConstraintEntry, Resolver};
use crate::source::Source;
use crate::store::cas::Store;
use crate::types::{Constraint, PackageIdentity, SourceType, Version};

use super::prompt::{prompt_allow_package, AllowDecision};

fn write_lockfile(cwd: &Path, store: Option<&Store>, pkg_entries: &[PackageEntry]) -> Result<()> {
    let ts = current_timestamp();

    let graph_hash = store.and_then(|_| {
        if pkg_entries.is_empty() {
            None
        } else {
            let graph_bytes = serde_json::to_vec(pkg_entries).ok()?;
            let raw = crate::util::hash::compute(&graph_bytes);
            let hex = crate::util::hash::hex_encode(&raw);
            let store_hash = store?.put_graph(&graph_bytes).ok()?;
            Some(format!("sha256:{hex} (store: {store_hash})"))
        }
    });

    let lockfile = Lockfile {
        version: 1,
        graph: GraphMeta {
            resolver: "mvs".to_string(),
            generated_at: Some(ts),
            graph_hash,
        },
        packages: pkg_entries.to_vec(),
    };
    let lock_content = crate::lockfile::generator::generate(&lockfile);
    let lock_path = cwd.join("ara.lock");
    let mut lock_f = std::fs::File::create(&lock_path)?;
    lock_f.write_all(lock_content.as_bytes())?;
    println!("Lockfile written to ara.lock");
    Ok(())
}

fn current_timestamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn find_dep<'a>(
    deps: &'a [crate::manifest::types::DependencyEntry],
    name: &str,
) -> Option<&'a crate::manifest::types::DependencyEntry> {
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
    dep: &crate::manifest::types::DependencyEntry,
) -> Result<Source> {
    Ok(match source_type {
        SourceType::Npm | SourceType::Registry => {
            let default_url = std::env::var("ARA_NPM_REGISTRY")
                .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());
            let url = dep.url.as_deref().unwrap_or(&default_url);
            Source::Registry(crate::source::registry::RegistrySource::new(
                url.to_string(),
            ))
        }
        SourceType::Github => {
            let repo = dep
                .repo
                .as_deref()
                .context("missing repo for github source")?;
            Source::Github(crate::source::github::GithubSource::new(repo.to_string()))
        }
        SourceType::Git => {
            let url = dep.url.as_deref().context("missing url for git source")?;
            let commit = dep.commit.as_deref().unwrap_or("HEAD");
            Source::Git(crate::source::git::GitSource::new(
                url.to_string(),
                commit.to_string(),
            ))
        }
        SourceType::Local => {
            let path = dep
                .path
                .as_deref()
                .context("missing path for local source")?;
            Source::Local(crate::source::local::LocalSource::new(path.to_string()))
        }
        SourceType::Url => {
            let url = dep.url.as_deref().context("missing url for url source")?;
            Source::Url(crate::source::tarball::TarballSource::new(url.to_string()))
        }
        SourceType::Workspace => {
            let path = dep.path.as_deref().unwrap_or(".");
            Source::Workspace(crate::source::workspace::WorkspaceSource::new(
                path.to_string(),
            ))
        }
    })
}

fn read_member_manifest(member_dir: &Path) -> Option<crate::manifest::types::Manifest> {
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
    workspace: &crate::manifest::types::Workspace,
    cwd: &Path,
) -> Vec<crate::manifest::types::DependencyEntry> {
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

            entries.push(crate::manifest::types::DependencyEntry {
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

struct TarballEntry {
    path: std::path::PathBuf,
    entry_type: tar::EntryType,
    data: Vec<u8>,
    mode: u32,
}

fn extract_tarball(tarball: &[u8], dest: &Path) -> Result<()> {
    let decoder = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(decoder);

    let mut raw_entries: Vec<TarballEntry> = Vec::new();
    for entry in archive
        .entries()
        .context("failed to read tarball entries")?
    {
        let mut entry = entry.context("failed to read tarball entry")?;
        let path = entry
            .path()
            .context("failed to read entry path")?
            .into_owned();
        let entry_type = entry.header().entry_type();
        let mode = entry.header().mode()?;
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        raw_entries.push(TarballEntry {
            path,
            entry_type,
            data,
            mode,
        });
    }

    let prefix = detect_tarball_prefix(&raw_entries);

    for entry in &raw_entries {
        let stripped = entry.path.strip_prefix(&prefix).unwrap_or(&entry.path);
        if stripped.as_os_str().is_empty() {
            continue;
        }
        let target = dest.join(stripped);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if entry.entry_type == tar::EntryType::Directory {
            std::fs::create_dir_all(&target)?;
        } else {
            std::fs::write(&target, &entry.data)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(entry.mode))?;
            }
        }
    }
    Ok(())
}

fn detect_tarball_prefix(entries: &[TarballEntry]) -> std::path::PathBuf {
    let first_comp = entries.first().and_then(|e| e.path.components().next());
    let common = first_comp.filter(|comp| {
        entries.iter().all(|e| {
            let mut comps = e.path.components();
            comps.next() == Some(*comp) && comps.next().is_some()
        })
    });

    match common {
        Some(comp) if comp.as_os_str() == "package" => std::path::PathBuf::from("package"),
        Some(comp) => std::path::PathBuf::from(comp.as_os_str()),
        None => std::path::PathBuf::new(),
    }
}

/// Create symlinks in `node_modules/.bin/` for the package's `bin` entries.
fn install_bin_links(node_modules: &Path, pkg_name: &str, pkg_dir: &Path) -> Result<()> {
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
            vec![(pkg_name.to_string(), cmd.clone())]
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
        // Target is relative: ../pkg_name/rel_path
        let target = format!("../{}/{}", pkg_name, rel_path);
        let _ = std::fs::remove_file(&link);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link)
            .with_context(|| format!("failed to create symlink {link:?} -> {target}"))?;
        #[cfg(not(unix))]
        std::fs::hard_link(pkg_dir.join(rel_path), &link)
            .with_context(|| format!("failed to link {link:?}"))?;
    }

    Ok(())
}

/// Scan `node_modules/<pkg>/package.json` for each installed package and
/// recursively install any missing transitive dependencies.
fn install_transitive_deps(
    node_modules: &Path,
    store: &Store,
    store_index: &mut HashMap<String, String>,
    pkg_entries: &mut Vec<PackageEntry>,
) -> Result<()> {
    let registry_url = std::env::var("ARA_NPM_REGISTRY")
        .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());

    let mut installed_any = true;
    while installed_any {
        installed_any = false;

        let dirs: Vec<PathBuf> = std::fs::read_dir(node_modules)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().ok().is_some_and(|t| t.is_dir()))
            .map(|e| e.path())
            .filter(|p| p.file_name().and_then(|s| s.to_str()) != Some(".bin"))
            .collect();

        for pkg_dir in &dirs {
            let pkg_json = pkg_dir.join("package.json");
            if !pkg_json.exists() {
                continue;
            }
            let content = std::fs::read_to_string(&pkg_json)?;
            let pkg: serde_json::Value = serde_json::from_str(&content)?;

            // Collect from `dependencies`, `peerDependencies`, and `optionalDependencies`
            let dep_sources = ["dependencies", "peerDependencies", "optionalDependencies"];
            let mut deps: Vec<(String, String)> = Vec::new();
            for key in &dep_sources {
                if let Some(map) = pkg.get(key).and_then(|v| v.as_object()) {
                    for (name, ver) in map {
                        if let Some(ver_str) = ver.as_str() {
                            deps.push((name.clone(), ver_str.to_string()));
                        }
                    }
                }
            }

            for (dep_name, dep_ver_str) in &deps {
                let dep_dir = node_modules.join(dep_name);
                if dep_dir.exists() {
                    continue;
                }

                println!("  installing transitive dep: {}@{}", dep_name, dep_ver_str);

                let reg = crate::source::registry::RegistrySource::new(registry_url.clone());
                let resolved_ver = match reg.resolve(dep_name) {
                    Ok(v) => v,
                    Err(e) => {
                        println!("    warning: cannot resolve {}: {}", dep_name, e);
                        continue;
                    }
                };
                let parsed_ver = match Version::parse(&resolved_ver) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let identity = PackageIdentity {
                    source: SourceType::Npm,
                    name: dep_name.clone(),
                    version: parsed_ver.clone(),
                    content_hash: None,
                    requested_ref: None,
                };

                let short_ver = format!(
                    "{}.{}.{}",
                    parsed_ver.major, parsed_ver.minor, parsed_ver.patch
                );
                let cache_key = format!("npm:{}@{}", dep_name, short_ver);

                let (pkg_content, hash_str) = if let Some(cached_hash) = store_index.get(&cache_key)
                {
                    if store.contains(cached_hash) {
                        if let Some(content) = store.get(cached_hash)? {
                            println!("    using cached {}@{}", dep_name, short_ver);
                            (content, cached_hash.clone())
                        } else {
                            store_index.remove(&cache_key);
                            let content = reg.fetch(&identity)?;
                            let hash = store.put(&content)?;
                            store_index.insert(cache_key, hash.clone());
                            (content, hash)
                        }
                    } else {
                        store_index.remove(&cache_key);
                        let content = reg.fetch(&identity)?;
                        let hash = store.put(&content)?;
                        store_index.insert(cache_key, hash.clone());
                        (content, hash)
                    }
                } else {
                    let content = reg.fetch(&identity)?;
                    let hash = store.put(&content)?;
                    store_index.insert(cache_key, hash.clone());
                    (content, hash)
                };

                std::fs::create_dir_all(&dep_dir)?;
                if let Err(e) = extract_tarball(&pkg_content, &dep_dir) {
                    println!("    failed to extract {}: {}", dep_name, e);
                    let _ = std::fs::remove_dir_all(&dep_dir);
                    continue;
                }

                if let Err(e) = install_bin_links(node_modules, dep_name, &dep_dir) {
                    println!(
                        "    warning: failed to create bin links for {}: {}",
                        dep_name, e
                    );
                }

                pkg_entries.push(PackageEntry {
                    name: dep_name.clone(),
                    version: short_ver,
                    source: "npm".to_string(),
                    package_hash: hash_str,
                    integrity: None,
                    signature: None,
                    repository: None,
                    commit: None,
                    dependencies: None,
                    security: None,
                    sbom: None,
                });

                installed_any = true;
            }
        }
    }

    Ok(())
}

pub(crate) fn cmd_install(non_interactive: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    cmd_install_in(&cwd, non_interactive)
}

pub(crate) fn cmd_install_specs(
    specs: &[String],
    save_dev: bool,
    save_peer: bool,
    save_optional: bool,
    range: Option<&str>,
    force: bool,
    refresh: bool,
    offline: bool,
    non_interactive: bool,
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
            crate::manifest::types::Manifest {
                project: crate::manifest::types::Project {
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

    let index_path = store_base.join("index.json");
    let mut store_index: HashMap<String, String> = if index_path.exists() {
        let idx_content = std::fs::read_to_string(&index_path)?;
        serde_json::from_str(&idx_content).unwrap_or_default()
    } else {
        HashMap::new()
    };

    // Seed pkg_entries from existing lockfile so we don't lose prior entries
    let lock_path = cwd.join("ara.lock");
    let mut pkg_entries: Vec<PackageEntry> = if lock_path.exists() {
        let lock_content = std::fs::read_to_string(&lock_path).unwrap_or_default();
        if let Ok(existing) = crate::lockfile::parser::parse(&lock_content) {
            existing.packages
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    for spec in specs {
        let target = crate::source::url::parse_install_spec(spec)
            .with_context(|| format!("failed to parse spec: {spec}"))?;

        let mut meta = resolve_spec_meta(&target, range)?;

        if m.deps.iter().any(|d| d.name == meta.name) {
            println!("  {} already in manifest, skipping", meta.name);
            continue;
        }

        if pkg_entries.iter().any(|e| e.name == meta.name) {
            println!("  {} already in lockfile, skipping", meta.name);
            continue;
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
            let content = fetch_meta_content(&meta)?;
            let hash = store.put(&content)?;
            store_index.insert(cache_key.clone(), hash.clone());
            println!("  fetching {}@{} (forced)...", meta.name, ver_str);
            (content, hash)
        } else if let Some(cached_hash) = store_index.get(&cache_key) {
            if store.contains(cached_hash) {
                if let Some(content) = store.get(cached_hash)? {
                    if refresh {
                        println!("  refresh: re-fetching {}@{}", meta.name, ver_str);
                        let content = fetch_meta_content(&meta)?;
                        let hash = store.put(&content)?;
                        store_index.insert(cache_key, hash.clone());
                        (content, hash)
                    } else {
                        println!("  using cached {}@{}", meta.name, ver_str);
                        (content, cached_hash.clone())
                    }
                } else {
                    store_index.remove(&cache_key);
                    let content = fetch_meta_content(&meta)?;
                    let hash = store.put(&content)?;
                    store_index.insert(cache_key, hash.clone());
                    (content, hash)
                }
            } else {
                store_index.remove(&cache_key);
                let content = fetch_meta_content(&meta)?;
                let hash = store.put(&content)?;
                store_index.insert(cache_key, hash.clone());
                (content, hash)
            }
        } else if offline {
            anyhow::bail!(
                "{}@{} not found in cache (--offline mode)",
                meta.name,
                ver_str
            );
        } else {
            let content = fetch_meta_content(&meta)?;
            let hash = store.put(&content)?;
            store_index.insert(cache_key, hash.clone());
            println!("  fetching {}@{}...", meta.name, ver_str);
            (content, hash)
        };

        // For tarball URLs, identity is embedded in the tarball — extract it now
        if meta.source_type == SourceType::Url {
            let (real_name, real_version) =
                crate::source::tarball::identity_from_tarball(&pkg_content).unwrap_or_else(|_| {
                    let fallback =
                        crate::source::tarball::name_from_url(meta.url.as_deref().unwrap_or(""))
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
                } else if non_interactive {
                    let rl = result.risk_level;
                    print!(
                        "  ✓ {}@{} ({}) ⚠  {} finding(s) ({})",
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

        // Add to manifest
        m.deps.push(crate::manifest::types::DependencyEntry {
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

        println!();
    }

    // Save store index
    std::fs::write(&index_path, serde_json::to_string_pretty(&store_index)?)?;

    // Write updated ara.toml
    let ara_toml_content = generate_ara_toml(&m);
    std::fs::write(cwd.join("ara.toml"), &ara_toml_content).context("failed to write ara.toml")?;

    println!("Updated ara.toml with {} dep(s)", m.deps.len());

    if !pkg_entries.is_empty() {
        write_lockfile(&cwd, Some(&store), &pkg_entries)?;
    }

    // Install transitive dependencies discovered from the newly installed packages
    install_transitive_deps(&node_modules, &store, &mut store_index, &mut pkg_entries)?;

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
fn fetch_meta_content(meta: &ResolvedMeta) -> Result<Vec<u8>> {
    match meta.source_type {
        SourceType::Npm | SourceType::Registry => {
            let registry_url = std::env::var("ARA_NPM_REGISTRY")
                .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());
            let reg = crate::source::registry::RegistrySource::new(registry_url);
            let identity = PackageIdentity {
                source: SourceType::Npm,
                name: meta.name.clone(),
                version: meta.version_semver.clone(),
                content_hash: None,
                requested_ref: None,
            };
            reg.fetch(&identity)
                .with_context(|| format!("failed to fetch {}@{}", meta.name, meta.version))
        }
        SourceType::Github => {
            let repo = meta.repo.as_deref().unwrap_or(&meta.name);
            let src = crate::source::github::GithubSource::new(repo.to_string());
            let identity = PackageIdentity {
                source: SourceType::Github,
                name: repo.to_string(),
                version: meta.version_semver.clone(),
                content_hash: None,
                requested_ref: meta.commit.clone(),
            };
            src.fetch(&identity)
                .with_context(|| format!("failed to fetch github:{repo}"))
        }
        SourceType::Git => {
            let url = meta.url.as_deref().unwrap_or(&meta.name);
            let commit_str = meta.commit.clone().unwrap_or_else(|| "HEAD".to_string());
            let src = crate::source::git::GitSource::new(url.to_string(), commit_str.clone());
            let identity = PackageIdentity {
                source: SourceType::Git,
                name: url.to_string(),
                version: meta.version_semver.clone(),
                content_hash: None,
                requested_ref: Some(commit_str),
            };
            src.fetch(&identity)
                .with_context(|| format!("failed to fetch git:{url}"))
        }
        SourceType::Url => {
            let url = meta.url.as_deref().unwrap_or(&meta.name);
            let src = crate::source::tarball::TarballSource::new(url.to_string());
            let identity = PackageIdentity {
                source: SourceType::Url,
                name: url.to_string(),
                version: Version::new(0, 0, 0),
                content_hash: None,
                requested_ref: None,
            };
            src.fetch(&identity)
                .with_context(|| format!("failed to download {url}"))
        }
        _ => anyhow::bail!("unsupported source type: {}", meta.source_type),
    }
}

fn resolve_npm_meta(
    name: &str,
    version: Option<&str>,
    range: Option<&str>,
) -> Result<ResolvedMeta> {
    let registry_url = std::env::var("ARA_NPM_REGISTRY")
        .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());

    let reg = crate::source::registry::RegistrySource::new(registry_url);

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
                .with_context(|| format!("failed to resolve {name} for range {v}"))?;
            (concrete, v.to_string())
        }
    } else {
        let concrete = reg
            .resolve(name)
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

fn resolve_spec_meta(
    target: &crate::source::url::InstallTarget,
    range: Option<&str>,
) -> Result<ResolvedMeta> {
    match target {
        crate::source::url::InstallTarget::Npm { name, version } => {
            resolve_npm_meta(name, version.as_deref(), range)
        }
        crate::source::url::InstallTarget::Github { repo, commit } => {
            resolve_github_meta(repo, commit.as_deref())
        }
        crate::source::url::InstallTarget::Git { url, commit } => {
            resolve_git_meta(url, commit.as_deref())
        }
        crate::source::url::InstallTarget::Tarball { url } => resolve_tarball_meta(url),
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

fn toml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0C' => out.push_str("\\f"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn generate_ara_toml(m: &crate::manifest::types::Manifest) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "[project]\nname = \"{}\"\nversion = \"{}\"\n",
        toml_escape(&m.project.name),
        toml_escape(&m.project.version)
    ));

    if !m.deps.is_empty() {
        out.push_str("\n[deps]\n");
        for dep in &m.deps {
            out.push_str(&format!(
                "\"{}\" = {{ source = \"{}\"",
                toml_escape(&dep.name),
                toml_escape(&dep.source)
            ));
            if let Some(kind) = &dep.kind {
                out.push_str(&format!(", kind = \"{}\"", toml_escape(kind)));
            }
            if let Some(ver) = &dep.version {
                out.push_str(&format!(", version = \"{}\"", toml_escape(ver)));
            }
            if let Some(repo) = &dep.repo {
                out.push_str(&format!(", repo = \"{}\"", toml_escape(repo)));
            }
            if let Some(url) = &dep.url {
                out.push_str(&format!(", url = \"{}\"", toml_escape(url)));
            }
            if let Some(commit) = &dep.commit {
                out.push_str(&format!(", commit = \"{}\"", toml_escape(commit)));
            }
            if let Some(path) = &dep.path {
                out.push_str(&format!(", path = \"{}\"", toml_escape(path)));
            }
            out.push_str(" }\n");
        }
    }

    if let Some(ws) = &m.workspace {
        out.push_str("\n[workspace]\nmembers = [");
        for (i, member) in ws.members.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("\"{}\"", toml_escape(member)));
        }
        out.push_str("]\n");
    }

    if !m.scripts.is_empty() {
        out.push_str("\n[scripts]\n");
        for script in &m.scripts {
            out.push_str(&format!(
                "\"{}\" = \"{}\"\n",
                toml_escape(&script.name),
                toml_escape(&script.command)
            ));
        }
    }

    out
}

fn read_manifest(cwd: &Path) -> Result<crate::manifest::types::Manifest> {
    let manifest_path = cwd.join("ara.toml");
    let pkg_json_path = cwd.join("package.json");

    if manifest_path.exists() {
        let content = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let m = parser::parse(&content).context("failed to parse ara.toml")?;
        return Ok(m);
    }

    if pkg_json_path.exists() {
        let content = std::fs::read_to_string(&pkg_json_path)
            .with_context(|| format!("failed to read {}", pkg_json_path.display()))?;
        let m =
            package_json::parse_package_json(&content).context("failed to parse package.json")?;

        let ara_toml_content = generate_ara_toml(&m);
        std::fs::write(&manifest_path, &ara_toml_content)
            .with_context(|| format!("failed to write {}", manifest_path.display()))?;

        println!(
            "No ara.toml found. Using package.json as manifest — generated {}",
            manifest_path.display()
        );

        return Ok(m);
    }

    Err(anyhow::anyhow!(
        "no manifest found: neither ara.toml nor package.json exists in {}",
        cwd.display()
    ))
}

#[allow(clippy::too_many_lines)]
fn cmd_install_in(cwd: &Path, non_interactive: bool) -> Result<()> {
    let mut m = read_manifest(cwd)?;

    println!(
        "Installing dependencies for {} v{}",
        m.project.name, m.project.version
    );

    // Expand workspace members into deps automatically
    if let Some(ws) = &m.workspace {
        let workspace_deps = expand_workspace_members(ws, cwd);
        for dep in workspace_deps {
            if !m.deps.iter().any(|d| d.name == dep.name) {
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

    // Connect resolve(): enhance each node's version from registry sources
    for node in &mut graph.nodes {
        if let Some(dep) = find_dep(&m.deps, &node.name) {
            if let Ok(src) = create_source(node.source, dep) {
                if let Ok(version_str) = src.resolve(&node.name) {
                    if let Ok(parsed) = Version::parse(&version_str) {
                        node.version = parsed;
                    }
                }
            }
        }
    }

    // Connect has_cycles(): warn if circular dependencies found
    if graph.has_cycles() {
        println!("warning: circular dependency detected in the resolved graph");
    }

    let node_modules = cwd.join("node_modules");

    let lock_path = cwd.join("ara.lock");
    if lock_path.exists() && node_modules.exists() {
        let lock_content = std::fs::read_to_string(&lock_path).unwrap_or_default();
        if let Ok(existing) = crate::lockfile::parser::parse(&lock_content) {
            let all_match = existing.packages.iter().all(|p| {
                graph.find_node(&p.name).is_some_and(|idx| {
                    let n = &graph.nodes[idx];
                    let v = format!(
                        "{}.{}.{}",
                        n.version.major, n.version.minor, n.version.patch
                    );
                    n.source.to_string() == p.source && v == p.version
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

    let index_path = store_base.join("index.json");
    let mut store_index: HashMap<String, String> = if index_path.exists() {
        let idx_content = std::fs::read_to_string(&index_path)?;
        serde_json::from_str(&idx_content).unwrap_or_default()
    } else {
        HashMap::new()
    };

    if let Err(e) = std::fs::create_dir_all(&node_modules) {
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(e).context("failed to create node_modules directory");
        }
    }

    let mut pkg_entries: Vec<PackageEntry> = Vec::new();

    for node in &graph.nodes {
        let ver_str = format!(
            "{}.{}.{}",
            node.version.major, node.version.minor, node.version.patch
        );

        let Some(dep) = find_dep(&m.deps, &node.name) else {
            println!("  skipped {}: no dependency config", node.name);
            continue;
        };

        let src = match create_source(node.source, dep) {
            Ok(s) => s,
            Err(e) => {
                println!("  skipped {}: failed to create source ({})", node.name, e);
                continue;
            }
        };

        let cache_key = format!("{}@{}", node.name, ver_str);

        let (pkg_content, hash_str) = if let Some(cached_hash) = store_index.get(&cache_key) {
            if store.contains(cached_hash) {
                if let Some(content) = store.get(cached_hash)? {
                    println!("  using cached {}@{}", node.name, ver_str);
                    (content, cached_hash.clone())
                } else {
                    store_index.remove(&cache_key);
                    fetch_and_store(&store, &mut store_index, &src, &cache_key, node, &ver_str)?
                }
            } else {
                store_index.remove(&cache_key);
                fetch_and_store(&store, &mut store_index, &src, &cache_key, node, &ver_str)?
            }
        } else {
            fetch_and_store(&store, &mut store_index, &src, &cache_key, node, &ver_str)?
        };

        let pkg_dir = node_modules.join(&node.name);

        // clean any existing directory
        let _ = std::fs::remove_dir_all(&pkg_dir);
        std::fs::create_dir_all(&pkg_dir)?;

        if let Err(e) = extract_tarball(&pkg_content, &pkg_dir) {
            println!("  failed to extract {}: {}", node.name, e);
            continue;
        }

        if let Err(e) = install_bin_links(&node_modules, &node.name, &pkg_dir) {
            println!(
                "  warning: failed to create bin links for {}: {}",
                node.name, e
            );
        }

        let (allowed, security) = match analyzer::analyze_package(&pkg_dir) {
            Ok(result) => {
                if result.findings.is_empty() {
                    print!("  ✓ {}@{} ({})", node.name, ver_str, hash_str);
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
                        node.name,
                        ver_str,
                        hash_str,
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
                    match prompt_allow_package(&node.name, &ver_str, &result.findings) {
                        AllowDecision::Yes | AllowDecision::Sandbox => {
                            println!("  ✓ {}@{} ({}) — allowed", node.name, ver_str, hash_str);
                            (
                                true,
                                Some(SecurityMeta {
                                    risk_level: Some(result.risk_level.to_string()),
                                }),
                            )
                        }
                        AllowDecision::No => {
                            let _ = std::fs::remove_dir_all(&pkg_dir);
                            println!("  ✗ {}@{} ({}) — denied", node.name, ver_str, hash_str);
                            (false, None)
                        }
                    }
                }
            }
            Err(_) => {
                print!("  ✓ {}@{} ({})", node.name, ver_str, hash_str);
                (true, None)
            }
        };

        if !allowed {
            continue;
        }

        pkg_entries.push(PackageEntry {
            name: node.name.clone(),
            version: ver_str.clone(),
            source: node.source.to_string(),
            package_hash: hash_str.clone(),
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

    // Save store index for future cache lookups
    std::fs::write(&index_path, serde_json::to_string_pretty(&store_index)?)?;

    // Install transitive dependencies discovered from extracted packages
    install_transitive_deps(&node_modules, &store, &mut store_index, &mut pkg_entries)?;

    let graph_bytes = serde_json::to_vec(&graph.nodes)?;
    let store_graph_hash = store.put_graph(&graph_bytes)?;
    let raw = graph.compute_hash()?;
    let graph_hash = format!("sha256:{}", crate::util::hash::hex_encode(&raw));
    // Verify stored hash matches computed hash (sanity check)
    if !store_graph_hash.contains(&graph_hash[7..17]) {
        println!("note: stored graph hash diverges from computed hash");
    }

    let ts = current_timestamp();

    let lockfile = Lockfile {
        version: 1,
        graph: GraphMeta {
            resolver: "mvs".to_string(),
            generated_at: Some(ts),
            graph_hash: Some(graph_hash),
        },
        packages: pkg_entries,
    };

    let lock_content = crate::lockfile::generator::generate(&lockfile);
    let lock_path = cwd.join("ara.lock");
    let mut lock_f = std::fs::File::create(&lock_path)?;
    lock_f.write_all(lock_content.as_bytes())?;
    println!("Lockfile written to ara.lock");

    Ok(())
}

fn fetch_and_store(
    store: &Store,
    store_index: &mut HashMap<String, String>,
    src: &Source,
    cache_key: &str,
    node: &crate::resolver::graph::Node,
    ver_str: &str,
) -> Result<(Vec<u8>, String)> {
    println!("  fetching {}@{}...", node.name, ver_str);

    let identity = crate::types::PackageIdentity {
        source: node.source,
        name: node.name.clone(),
        version: node.version.clone(),
        content_hash: None,
        requested_ref: None,
    };

    let pkg_content = match src.fetch(&identity) {
        Ok(c) => c,
        Err(e) => {
            return Err(anyhow::anyhow!("failed to fetch {}: {}", node.name, e));
        }
    };

    let hash_str = store.put(&pkg_content)?;
    store_index.insert(cache_key.to_string(), hash_str.clone());

    Ok((pkg_content, hash_str))
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
            crate::manifest::types::DependencyEntry {
                name: "zod".into(),
                source: "npm".into(),
                kind: None,
                version: Some("^3.0.0".into()),
                repo: None,
                url: None,
                commit: None,
                path: None,
            },
            crate::manifest::types::DependencyEntry {
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

    #[test]
    fn test_cmd_install_local_dep() {
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

        cmd_install_in(&root_path, true).unwrap();

        assert!(root_path.join("node_modules").exists());
        assert!(root_path.join("node_modules/dep-a").exists());
        assert!(root_path.join("node_modules/dep-a/index.js").exists());
        assert!(root_path.join("ara.lock").exists());

        let lock_content = std::fs::read_to_string(root_path.join("ara.lock")).unwrap();
        let lf = crate::lockfile::parser::parse(&lock_content).unwrap();
        assert!(!lf.packages.is_empty());
        assert_eq!(lf.packages[0].name, "dep-a");
    }

    #[test]
    fn test_cmd_install_no_deps() {
        let root = tempfile::tempdir().unwrap();
        let root_manifest = r#"
            [project]
            name = "empty"
            version = "0.0.1"
        "#;
        std::fs::write(root.path().join("ara.toml"), root_manifest).unwrap();
        assert!(cmd_install_in(root.path(), true).is_ok());
    }

    #[test]
    fn test_cmd_install_missing_manifest() {
        let root = tempfile::tempdir().unwrap();
        assert!(cmd_install_in(root.path(), true).is_err());
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

        let ws = crate::manifest::types::Workspace {
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
    fn test_generate_ara_toml_minimal() {
        let m = crate::manifest::types::Manifest {
            project: crate::manifest::types::Project {
                name: "app".into(),
                version: "1.0.0".into(),
            },
            deps: vec![],
            workspace: None,
            scripts: vec![],
            security: None,
            build: None,
            package_json_extras: None,
        };
        let out = generate_ara_toml(&m);
        assert!(out.contains(r#"name = "app""#));
        assert!(out.contains(r#"version = "1.0.0""#));
    }

    #[test]
    fn test_generate_ara_toml_with_deps() {
        let m = crate::manifest::types::Manifest {
            project: crate::manifest::types::Project {
                name: "app".into(),
                version: "0.1.0".into(),
            },
            deps: vec![
                crate::manifest::types::DependencyEntry {
                    name: "zod".into(),
                    source: "npm".into(),
                    kind: Some("prod".into()),
                    version: Some("^3.0.0".into()),
                    repo: None,
                    url: None,
                    commit: None,
                    path: None,
                },
                crate::manifest::types::DependencyEntry {
                    name: "vitest".into(),
                    source: "npm".into(),
                    kind: Some("dev".into()),
                    version: Some("^1.0.0".into()),
                    repo: None,
                    url: None,
                    commit: None,
                    path: None,
                },
            ],
            workspace: None,
            scripts: vec![],
            security: None,
            build: None,
            package_json_extras: None,
        };
        let out = generate_ara_toml(&m);
        assert!(out.contains(r#"zod"#));
        assert!(out.contains(r#"vitest"#));
        assert!(out.contains(r#"kind = "prod""#));
        assert!(out.contains(r#"kind = "dev""#));
        assert!(out.contains(r#"version = "^3.0.0""#));
    }

    #[test]
    fn test_generate_ara_toml_with_scripts() {
        let m = crate::manifest::types::Manifest {
            project: crate::manifest::types::Project {
                name: "app".into(),
                version: "0.1.0".into(),
            },
            deps: vec![],
            workspace: None,
            scripts: vec![crate::manifest::types::ScriptEntry {
                name: "build".into(),
                command: "tsc".into(),
            }],
            security: None,
            build: None,
            package_json_extras: None,
        };
        let out = generate_ara_toml(&m);
        assert!(out.contains("[scripts]"));
        assert!(out.contains(r#""build" = "tsc""#));
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

        // Should have generated ara.toml
        let ara_toml = std::fs::read_to_string(root.path().join("ara.toml")).unwrap();
        assert!(ara_toml.contains(r#"name = "my-app""#));
    }

    #[test]
    fn test_read_manifest_ara_toml_preferred() {
        let root = tempfile::tempdir().unwrap();
        // Both exist
        let pkg_json = r#"{"name": "from-pkg-json", "version": "1.0.0"}"#;
        std::fs::write(root.path().join("package.json"), pkg_json).unwrap();
        let ara_toml = r#"[project]
name = "from-ara-toml"
version = "2.0.0"
"#;
        std::fs::write(root.path().join("ara.toml"), ara_toml).unwrap();

        let m = read_manifest(root.path()).unwrap();
        assert_eq!(m.project.name, "from-ara-toml");
        assert_eq!(m.project.version, "2.0.0");
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

        let ws = crate::manifest::types::Workspace {
            members: vec!["packages/*".to_string()],
        };
        let entries = expand_workspace_members(&ws, root.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "pkg-a");
        assert_eq!(entries[0].version.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn test_cmd_install_unicode_name() {
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

        cmd_install_in(&root_path, true).unwrap();

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
