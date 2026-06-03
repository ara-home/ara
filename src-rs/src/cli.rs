use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::analysis::analyzer;
use crate::lockfile::types::{GraphMeta, Lockfile, PackageEntry, SecurityMeta};
use crate::manifest::parser;
use crate::resolver::mvs::{ConstraintEntry, Resolver};
use crate::sandbox::executor::Executor;
use crate::sandbox::profiles::{Profile, SandboxConfig};
use crate::source::Source;
use crate::store::cas::Store;
use crate::types::{Constraint, RiskLevel, SourceType, Version};

// ── CLI definition ─────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "ara", version, about = "Ara package manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Install project dependencies
    Install,
    /// Run a script in a sandboxed environment
    Run {
        script: String,
        #[arg(long, default_value = "runtime")]
        profile: String,
    },
    /// Analyze a package for security patterns
    Analyze {
        #[arg(default_value = ".")]
        path: String,
    },
    /// Full security audit of a package
    Audit {
        #[arg(default_value = ".")]
        path: String,
    },
    /// Build the project (not yet implemented)
    Build,
    /// Publish the project (not yet implemented)
    Publish,
    /// Run garbage collection on the store (not yet implemented)
    Gc,
    /// Trust a package (not yet implemented)
    Trust {
        package: String,
    },
}

impl Cli {
    pub fn run(&self) -> Result<()> {
        match &self.command {
            Commands::Install => cmd_install(),
            Commands::Run { script, profile } => cmd_run(script, profile),
            Commands::Analyze { path } => cmd_analyze(path),
            Commands::Audit { path } => cmd_audit(path),
            Commands::Build => {
                eprintln!("ara build: not yet implemented");
                Ok(())
            }
            Commands::Publish => {
                eprintln!("ara publish: not yet implemented");
                Ok(())
            }
            Commands::Gc => cmd_gc(),
            Commands::Trust { package: _ } => {
                eprintln!("ara trust: not yet implemented");
                Ok(())
            }
        }
    }
}

// ── helpers ────────────────────────────────────────────────────────────────

fn severity_color(severity: &str) -> &'static str {
    match severity {
        "critical" => "\x1b[31m",
        "high" => "\x1b[33m",
        "medium" => "\x1b[36m",
        "low" => "\x1b[32m",
        _ => "\x1b[0m",
    }
}

fn severity_label(severity: &str) -> &'static str {
    match severity {
        "critical" => "CRITICAL",
        "high" => "HIGH",
        "medium" => "MEDIUM",
        "low" => "LOW",
        _ => "UNKNOWN",
    }
}

fn print_findings(findings: &[crate::types::Finding], risk_level: RiskLevel) {
    let reset = "\x1b[0m";
    for f in findings {
        let color = severity_color(&f.severity.to_string());
        let label = severity_label(&f.severity.to_string());
        let location = f.location.as_deref().unwrap_or("-");
        println!("  {color}{label:>8}{reset}  {:<20}  {:<25}  {}", f.pattern, location, f.description);
    }
    println!("\n  Risk level: {}{}{reset}", severity_color(&risk_level.to_string()), risk_level);
}

#[allow(clippy::cast_possible_wrap)]
fn current_timestamp() -> String {
    // Simple ISO 8601 timestamp (UTC)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Simple calendar calculation (for dates after 1970)
    let mut y = 1970i64;
    let mut remaining = days as i64;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = is_leap(y);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0usize;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining < i64::from(md) {
            m = i;
            break;
        }
        remaining -= i64::from(md);
    }
    let day = remaining + 1;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m + 1,
        day,
        hours,
        minutes,
        seconds
    )
}

const fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn find_dep<'a>(deps: &'a [crate::manifest::types::DependencyEntry], name: &str) -> Option<&'a crate::manifest::types::DependencyEntry> {
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

fn create_source(source_type: SourceType, dep: &crate::manifest::types::DependencyEntry) -> Result<Source> {
    Ok(match source_type {
        SourceType::Npm => {
            let url = dep.url.as_deref().unwrap_or("https://registry.npmjs.org");
            Source::Npm(crate::source::registry::RegistrySource::new(url.to_string()))
        }
        SourceType::Registry => {
            let url = dep.url.as_deref().unwrap_or("https://registry.npmjs.org");
            Source::Registry(crate::source::registry::RegistrySource::new(url.to_string()))
        }
        SourceType::Github => {
            let repo = dep.repo.as_deref().context("missing repo for github source")?;
            Source::Github(crate::source::github::GithubSource::new(repo.to_string()))
        }
        SourceType::Git => {
            let url = dep.url.as_deref().context("missing url for git source")?;
            let commit = dep.commit.as_deref().unwrap_or("HEAD");
            Source::Git(crate::source::git::GitSource::new(url.to_string(), commit.to_string()))
        }
        SourceType::Local => {
            let path = dep.path.as_deref().context("missing path for local source")?;
            Source::Local(crate::source::local::LocalSource::new(path.to_string()))
        }
        SourceType::Workspace => {
            let path = dep.path.as_deref().unwrap_or(".");
            Source::Workspace(crate::source::workspace::WorkspaceSource::new(path.to_string()))
        }
    })
}

fn extract_tarball(tarball: &[u8], dest: &Path) -> Result<()> {
    let decoder = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(decoder);
    for entry in archive.entries().context("failed to read tarball entries")? {
        let mut entry = entry.context("failed to read tarball entry")?;
        let path = entry.path().context("failed to read entry path")?;
        let components: Vec<_> = path.components().collect();
        let stripped = if components.first().map_or(false, |c| c.as_os_str() == "package") {
            components.iter().skip(1).collect::<PathBuf>()
        } else {
            path.to_path_buf()
        };
        if stripped.as_os_str().is_empty() {
            continue;
        }
        let target = dest.join(&stripped);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        entry.unpack(target)?;
    }
    Ok(())
}

// ── analyze command ────────────────────────────────────────────────────────

fn cmd_analyze(path: &str) -> Result<()> {
    let abs_path = std::fs::canonicalize(path).context("invalid path")?;
    println!("Analyzing {}...\n", abs_path.display());

    match analyzer::analyze_package(&abs_path) {
        Ok(result) => {
            if result.findings.is_empty() {
                println!("  No suspicious patterns detected.");
            } else {
                print_findings(&result.findings, result.risk_level);
            }
        }
        Err(e) => {
            eprintln!("  Analysis failed: {e}");
        }
    }
    Ok(())
}

// ── audit command ──────────────────────────────────────────────────────────

fn cmd_audit(path: &str) -> Result<()> {
    let abs_path = std::fs::canonicalize(path).context("invalid path")?;
    println!("Auditing {}...\n", abs_path.display());

    match analyzer::analyze_package(&abs_path) {
        Ok(result) => {
            let summary = if result.findings.is_empty() {
                "No issues found.".to_string()
            } else {
                format!("Found {} potential issue(s).", result.findings.len())
            };

            if result.findings.is_empty() {
                println!("  No suspicious patterns detected.");
            } else {
                print_findings(&result.findings, result.risk_level);
            }
            println!("\n  Summary: {summary}");
        }
        Err(e) => {
            eprintln!("  Audit failed: {e}");
        }
    }
    Ok(())
}

// ── install command ────────────────────────────────────────────────────────

fn cmd_install() -> Result<()> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    cmd_install_in(&cwd)
}

#[allow(clippy::too_many_lines)]
fn cmd_install_in(cwd: &std::path::Path) -> Result<()> {
    let manifest_path = cwd.join("ara.toml");

    let content = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("ara.toml not found at {}", manifest_path.display()))?;

    let m = parser::parse(&content).context("failed to parse ara.toml")?;

    println!("Installing dependencies for {} v{}", m.project.name, m.project.version);

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

    // Step 5: Check lockfile for fast-path (skip if already up-to-date)
    let lock_path = cwd.join("ara.lock");
    if lock_path.exists() && node_modules.exists() {
        let lock_content = std::fs::read_to_string(&lock_path).unwrap_or_default();
        if let Ok(existing) = crate::lockfile::parser::parse(&lock_content) {
            let all_match = existing.packages.iter().all(|p| {
                graph.find_node(&p.name).is_some_and(|idx| {
                    let n = &graph.nodes[idx];
                    let v = format!("{}.{}.{}", n.version.major, n.version.minor, n.version.patch);
                    n.source.to_string() == p.source && v == p.version
                })
            });
            if all_match && !graph.nodes.is_empty() {
                let all_exist = graph.nodes.iter().all(|n| node_modules.join(&n.name).exists());
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

    // Step 2: Load store index for cache lookups
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
        let ver_str = format!("{}.{}.{}", node.version.major, node.version.minor, node.version.patch);

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

        // Step 2: Check store cache before fetching
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

        // Step 4: Analyze package and capture security info
        let security = if let Ok(result) = analyzer::analyze_package(&pkg_dir) {
            if result.findings.is_empty() {
                print!("  ✓ {}@{} ({})", node.name, ver_str, hash_str);
                Some(SecurityMeta {
                    risk_level: Some(result.risk_level.to_string()),
                    analysis_version: Some("1.0.0".to_string()),
                })
            } else {
                let rl = result.risk_level;
                let first = &result.findings[0];
                let loc = first.location.as_deref().unwrap_or("");
                print!("  ✓ {}@{} ({}) ⚠  {} finding(s) ({}) — {} in {}",
                    node.name, ver_str, hash_str, result.findings.len(), rl, first.description, loc);
                Some(SecurityMeta {
                    risk_level: Some(rl.to_string()),
                    analysis_version: Some("1.0.0".to_string()),
                })
            }
        } else {
            print!("  ✓ {}@{} ({})", node.name, ver_str, hash_str);
            None
        };

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

    // Step 3: Store graph and populate graph_hash using compute_hash()
    let graph_bytes = serde_json::to_vec(&graph.nodes)?;
    let store_graph_hash = store.put_graph(&graph_bytes)?;
    let raw = graph.compute_hash();
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

// ── gc command ─────────────────────────────────────────────────────────────

fn cmd_gc() -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let store_base = PathBuf::from(&home).join(".ara").join("store");
    cmd_gc_in(&store_base)
}

fn cmd_gc_in(store_base: &std::path::Path) -> Result<()> {
    let store = Store::new(store_base.to_path_buf());

    let index_path = store_base.join("index.json");

    // Read store index to find active hashes
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

// ── run command ────────────────────────────────────────────────────────────

fn cmd_run(script: &str, profile: &str) -> Result<()> {
    let profile: Profile = profile.parse().map_err(|e| anyhow::anyhow!("invalid profile: {e}"))?;
    println!("running: {script} ({profile:?})");
    let config = SandboxConfig::for_profile(profile);
    let executor = Executor::new(config);
    executor.execute(script)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_is_leap() {
        assert!(is_leap(2000));
        assert!(!is_leap(1900));
        assert!(is_leap(2024));
        assert!(!is_leap(2023));
        assert!(!is_leap(1970));
        assert!(is_leap(2004));
    }

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
                version: Some("^3.0.0".into()),
                repo: None,
                url: None,
                commit: None,
                path: None,
                package: None,
            },
            crate::manifest::types::DependencyEntry {
                name: "react".into(),
                source: "npm".into(),
                version: Some("^18.0.0".into()),
                repo: None,
                url: None,
                commit: None,
                path: None,
                package: None,
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
    fn test_severity_color() {
        assert_eq!(severity_color("critical"), "\x1b[31m");
        assert_eq!(severity_color("high"), "\x1b[33m");
        assert_eq!(severity_color("medium"), "\x1b[36m");
        assert_eq!(severity_color("low"), "\x1b[32m");
        assert_eq!(severity_color("unknown"), "\x1b[0m");
    }

    #[test]
    fn test_severity_label() {
        assert_eq!(severity_label("critical"), "CRITICAL");
        assert_eq!(severity_label("high"), "HIGH");
        assert_eq!(severity_label("medium"), "MEDIUM");
        assert_eq!(severity_label("low"), "LOW");
        assert_eq!(severity_label("unknown"), "UNKNOWN");
    }

    #[test]
    fn test_print_findings_does_not_crash() {
        let findings = vec![
            crate::types::Finding {
                pattern: "eval-usage".into(),
                severity: crate::types::RiskLevel::Critical,
                location: Some("index.js:1".into()),
                description: "eval detected".into(),
            },
        ];
        print_findings(&findings, crate::types::RiskLevel::Critical);
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

        cmd_install_in(&root_path).unwrap();

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
        assert!(cmd_install_in(root.path()).is_ok());
    }

    #[test]
    fn test_cmd_install_missing_manifest() {
        let root = tempfile::tempdir().unwrap();
        assert!(cmd_install_in(root.path()).is_err());
    }

    #[test]
    fn test_cmd_gc_clean_store() {
        let store_base = tempfile::tempdir().unwrap();
        let objects = store_base.path().join("objects");
        let graphs = store_base.path().join("graphs");
        std::fs::create_dir_all(&objects).unwrap();
        std::fs::create_dir_all(&graphs).unwrap();

        let mut index = std::collections::HashMap::new();
        index.insert("test-pkg@1.0.0".to_string(), "sha256-active".to_string());
        std::fs::write(store_base.path().join("index.json"), serde_json::to_string(&index).unwrap()).unwrap();

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
