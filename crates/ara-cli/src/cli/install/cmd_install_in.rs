use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};

use ara_analysis::analyzer;
use ara_lockfile::types::{GraphMeta, Lockfile, PackageEntry, SecurityMeta};
use ara_resolver::mvs::{ConstraintEntry, Resolver};
use ara_source::Source;
use ara_store::cas::Store;
use ara_store::index::StoreIndex;
use ara_types::{Constraint, SourceType, Version};

use super::disk_ops;
use super::lockfile;
use super::resolve;
use super::transitive;
use super::workspace;
use crate::cli::prompt::{prompt_allow_package, AllowDecision};

pub(crate) async fn cmd_install(non_interactive: bool, package_lock: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    cmd_install_in(&cwd, non_interactive, package_lock).await
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

fn find_dep<'a>(
    deps: &'a [ara_manifest::types::DependencyEntry],
    name: &str,
) -> Option<&'a ara_manifest::types::DependencyEntry> {
    deps.iter().find(|d| d.name == name)
}

#[allow(clippy::too_many_lines)]
async fn cmd_install_in(cwd: &Path, non_interactive: bool, package_lock: bool) -> Result<()> {
    let mut m = workspace::read_manifest(cwd)?;

    println!(
        "Installing dependencies for {} v{}",
        m.project.name, m.project.version
    );

    // Expand workspace members into deps automatically
    if let Some(ws) = &m.workspace {
        let workspace_deps = workspace::expand_workspace_members(ws, cwd);
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

    // Resolve catalog references in root deps and collect member catalog deps
    let _catalog_warnings = if let Some(ws) = &m.workspace {
        if let Some(ref cat) = ws.catalog {
            let cats = ws.catalogs.as_ref().cloned().unwrap_or_default();

            // Expand catalog refs in root dependencies
            let root_warnings =
                ara_manifest::catalog::resolve_catalog_refs(&mut m.deps, cat, &cats, "root")
                    .with_context(|| "failed to resolve catalog references in root dependencies")?;

            for w in &root_warnings {
                println!("  warning: {w}");
            }

            // Collect member deps with catalog expansion
            let member_catalog_deps =
                workspace::collect_member_deps_with_catalog(ws, cwd, cat, &cats);
            for dep in member_catalog_deps {
                if let Some(existing) = m.deps.iter_mut().find(|d| d.name == dep.name) {
                    if dep.version.is_some() {
                        existing.version = dep.version;
                    }
                } else {
                    m.deps.push(dep);
                }
            }

            root_warnings
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    if m.deps.is_empty() && m.workspace.is_none() {
        println!("No dependencies to install");
        lockfile::write_lockfile(cwd, None, &[], m.workspace.as_ref())
            .context("failed to write lockfile")?;
        if package_lock {
            lockfile::write_package_lock(cwd, &m, &[])?;
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
    let registry_url = std::env::var("ARA_NPM_REGISTRY")
        .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());
    if let Ok(reg) = ara_source::registry::RegistrySource::new(registry_url.clone()) {
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
                if let Ok(src) = resolve::create_source(source_type, dep) {
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

    let tasks: Vec<_> = tasks
        .into_iter()
        .map(|task| {
            let pb = pb_resolve.clone();
            async move {
                let r = task.await;
                pb.inc(1);
                r
            }
        })
        .collect();

    let results: Vec<_> = futures::future::join_all(tasks)
        .await
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
        let registry_url = registry_url.clone();

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

            let src = match resolve::create_source(node.source, dep) {
                Ok(s) => s,
                Err(_) => return None,
            };

            // Include registry URL in cache key to avoid collisions
            // between tarballs with the same name@version from different registries
            let registry_part = match node.source {
                SourceType::Npm | SourceType::Registry => {
                    let url = dep.url.as_deref().unwrap_or(&registry_url);
                    url.trim_end_matches('/')
                }
                _ => "",
            };
            let cache_key = if registry_part.is_empty() {
                format!("{}@{}", node.name, ver_str)
            } else {
                format!("{}:{}@{}", registry_part, node.name, ver_str)
            };

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
                    let (h, c) = resolve::fetch_and_store_parallel(
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
                let (h, c) = resolve::fetch_and_store_parallel(
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
                disk_ops::extract_package_cached(
                    &store_clone,
                    &hash_str_clone,
                    &pkg_dir_clone,
                    fresh_content,
                )
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

            if let Err(e) = disk_ops::install_bin_links(&node_modules, &node.name, &pkg_dir) {
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
    transitive::install_transitive_deps(
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

    let ts = lockfile::current_timestamp();

    if package_lock {
        lockfile::write_package_lock(cwd, &m, &pkg_entries)?;
    }

    let lockfile_workspace = m.workspace.as_ref().and_then(|ws| {
        if ws.catalog.is_none() && ws.catalogs.is_none() {
            None
        } else {
            Some(ara_lockfile::types::LockfileWorkspace {
                catalog: ws.catalog.clone(),
                catalogs: ws.catalogs.clone(),
            })
        }
    });

    let lockfile = Lockfile {
        version: 1,
        graph: GraphMeta {
            resolver: "mvs".to_string(),
            generated_at: Some(ts),
            graph_hash: Some(graph_hash),
        },
        workspace: lockfile_workspace,
        packages: pkg_entries,
    };

    let lock_content = ara_lockfile::generator::generate(&lockfile);
    let lock_path = cwd.join("ara.lock");
    let mut lock_f = std::fs::File::create(&lock_path)?;
    lock_f.write_all(lock_content.as_bytes())?;
    println!("Lockfile written to ara.lock");

    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

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
}
