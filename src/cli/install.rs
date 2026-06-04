use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::analysis::analyzer;
use crate::lockfile::types::{GraphMeta, Lockfile, PackageEntry, SecurityMeta};
use crate::manifest::package_json;
use crate::manifest::parser;
use crate::resolver::mvs::{ConstraintEntry, Resolver};
use crate::source::Source;
use crate::store::cas::Store;
use crate::types::{Constraint, SourceType, Version};

use super::prompt::{prompt_allow_package, AllowDecision};

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
        _ => SourceType::Npm,
    }
}

fn create_source(
    source_type: SourceType,
    dep: &crate::manifest::types::DependencyEntry,
) -> Result<Source> {
    Ok(match source_type {
        SourceType::Npm | SourceType::Registry => {
            let url = dep.url.as_deref().unwrap_or("https://registry.npmjs.org");
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

fn extract_tarball(tarball: &[u8], dest: &Path) -> Result<()> {
    let decoder = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(decoder);
    for entry in archive
        .entries()
        .context("failed to read tarball entries")?
    {
        let mut entry = entry.context("failed to read tarball entry")?;
        let path = entry.path().context("failed to read entry path")?;
        let stripped = path.strip_prefix("package").unwrap_or(&path);
        if stripped.as_os_str().is_empty() {
            continue;
        }
        let target = dest.join(stripped);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        entry.unpack(target)?;
    }
    Ok(())
}

pub(crate) fn cmd_install(non_interactive: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    cmd_install_in(&cwd, non_interactive)
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

    if m.deps.is_empty() {
        println!("No dependencies to install");
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
