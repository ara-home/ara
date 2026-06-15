use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};

use ara_analysis::analyzer;
use ara_lockfile::types::{PackageEntry, SecurityMeta};
use ara_store::cas::Store;
use ara_store::index::StoreIndex;
use ara_types::{RiskLevel, SourceType, Version};

use super::disk_ops;
use super::lockfile;
use super::resolve;
use super::transitive;
use super::workspace;
use crate::cli::prompt::{prompt_allow_package, AllowDecision};

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
    catalog: bool,
) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;

    // Read or bootstrap a minimal manifest
    let mut m = match workspace::read_manifest(&cwd) {
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
        if catalog {
            // Catalog mode: add a catalog reference without fetching
            let name = {
                let s = spec.trim();
                if let Some(at_pos) = s.find('@') {
                    s[..at_pos].to_string()
                } else if let Some(colon_pos) = s.find(':') {
                    s[..colon_pos].to_string()
                } else {
                    s.to_string()
                }
            };
            let catalog_version = {
                let s = spec.trim();
                if let Some(at_pos) = s.find('@') {
                    let after = &s[at_pos + 1..];
                    if after.is_empty() {
                        "catalog:".to_string()
                    } else {
                        format!("catalog:{after}")
                    }
                } else {
                    "catalog:".to_string()
                }
            };

            if let Some(pos) = m.deps.iter().position(|d| d.name == name) {
                m.deps.remove(pos);
            }

            println!("  catalog: {name} -> {catalog_version}");
            m.deps.push(ara_manifest::types::DependencyEntry {
                name,
                source: "npm".to_string(),
                kind: dep_kind.clone(),
                version: Some(catalog_version),
                repo: None,
                url: None,
                commit: None,
                path: None,
            });

            continue;
        }

        let target = ara_source::url::parse_install_spec(spec)
            .with_context(|| format!("failed to parse spec: {spec}"))?;

        let mut meta = resolve::resolve_spec_meta(&target, range).await?;

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
            let content = resolve::fetch_meta_content(&meta).await?;
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
                            let content = resolve::fetch_meta_content(&meta).await?;
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
                        let content = resolve::fetch_meta_content(&meta).await?;
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
                    let content = resolve::fetch_meta_content(&meta).await?;
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
                let content = resolve::fetch_meta_content(&meta).await?;
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

        if let Err(e) = disk_ops::extract_tarball(&pkg_content, &pkg_dir) {
            println!("  failed to extract {}: {}", meta.name, e);
            continue;
        }

        if let Err(e) = disk_ops::install_bin_links(&node_modules, &meta.name, &pkg_dir) {
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
                } else if result.risk_level <= RiskLevel::Medium {
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
                } else if non_interactive {
                    eprintln!(
                        "  ⚠  {}@{} ({}) — {} finding(s) ({}) — bypassed in non-interactive mode",
                        meta.name,
                        ver_str,
                        hash_str,
                        result.findings.len(),
                        result.risk_level
                    );
                    for finding in &result.findings {
                        let loc = finding.location.as_deref().unwrap_or("<unknown>");
                        eprintln!(
                            "    ⚠  [{}] {} — {}",
                            finding.severity, finding.pattern, loc
                        );
                        if !finding.description.is_empty() {
                            eprintln!("         {}", finding.description);
                        }
                    }
                    eprintln!(
                        "  tip: re-run interactively to review or use --force to install anyway"
                    );
                    let _ = std::fs::remove_dir_all(&pkg_dir);
                    (false, None)
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
            integrity: meta.integrity.clone(),
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
    transitive::install_transitive_deps(
        &node_modules,
        &store,
        &store_index,
        &mut pkg_entries,
        &installed_names,
    )
    .await?;

    if !pkg_entries.is_empty() {
        lockfile::write_lockfile(&cwd, Some(&store), &pkg_entries, m.workspace.as_ref())?;
        if package_lock {
            lockfile::write_package_lock(&cwd, &m, &pkg_entries)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[tokio::test]
    async fn test_cmd_install_unicode_name() {
        let root = tempfile::tempdir().unwrap();

        let pkg_json = r#"{"name": "unicode-test", "version": "1.0.0"}"#;
        std::fs::write(root.path().join("package.json"), pkg_json).unwrap();

        let result = cmd_install_specs(
            &["@test/unicode-pkg@^1.0.0".to_string()],
            false,
            false,
            false,
            None,
            false,
            false,
            false,
            true,
            false,
            false,
        )
        .await;
        assert!(result.is_err(), "expected failure, got: {result:?}");
    }
}
