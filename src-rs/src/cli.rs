use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::analysis::analyzer;
use crate::lockfile::types::{GraphMeta, Lockfile, PackageEntry};
use crate::manifest::parser;
use crate::resolver::mvs::{ConstraintEntry, Resolver};
use crate::source::Source;
use crate::store::cas::Store;
use crate::types::{Constraint, RiskLevel, SourceType};

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
    /// Run a script defined in ara.toml
    Run {
        script: String,
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
            Commands::Run { script } => cmd_run(script),
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
            Commands::Gc => {
                eprintln!("ara gc: not yet implemented");
                Ok(())
            }
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

fn print_findings(findings: &[crate::types::Finding], risk_level: &RiskLevel) {
    let reset = "\x1b[0m";
    for f in findings {
        let color = severity_color(&f.severity.to_string());
        let label = severity_label(&f.severity.to_string());
        let location = f.location.as_deref().unwrap_or("-");
        println!("  {color}{label:>8}{reset}  {:<20}  {:<25}  {}", f.pattern, location, f.description);
    }
    println!("\n  Risk level: {}{}{reset}", severity_color(&risk_level.to_string()), risk_level);
}

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
        if remaining < md as i64 {
            m = i;
            break;
        }
        remaining -= md as i64;
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

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn find_dep<'a>(deps: &'a [crate::manifest::types::DependencyEntry], name: &str) -> Option<&'a crate::manifest::types::DependencyEntry> {
    deps.iter().find(|d| d.name == name)
}

fn source_type_from_str(s: &str) -> SourceType {
    match s {
        "npm" | "registry" => SourceType::Npm,
        "github" => SourceType::Github,
        "git" => SourceType::Git,
        "local" => SourceType::Local,
        "workspace" => SourceType::Workspace,
        _ => SourceType::Npm,
    }
}

fn create_source(source_type: SourceType, dep: &crate::manifest::types::DependencyEntry) -> Result<Source> {
    Ok(match source_type {
        SourceType::Npm | SourceType::Registry => {
            let url = dep.url.as_deref().unwrap_or("https://registry.npmjs.org");
            Source::Npm(crate::source::registry::RegistrySource::new(url.to_string()))
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
    archive.unpack(dest).context("failed to extract tarball")?;
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
                print_findings(&result.findings, &result.risk_level);
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
                print_findings(&result.findings, &result.risk_level);
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

    let graph = r.resolve();
    println!("Resolved {} packages", graph.nodes.len());

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

    let mut pkg_entries: Vec<PackageEntry> = Vec::new();

    for node in &graph.nodes {
        let ver_str = format!("{}.{}.{}", node.version.major, node.version.minor, node.version.patch);

        let dep = match find_dep(&m.deps, &node.name) {
            Some(d) => d,
            None => {
                println!("  skipped {}: no dependency config", node.name);
                continue;
            }
        };

        let src = match create_source(node.source, dep) {
            Ok(s) => s,
            Err(e) => {
                println!("  skipped {}: failed to create source ({})", node.name, e);
                continue;
            }
        };

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
                println!("  failed to fetch {}: {}", node.name, e);
                continue;
            }
        };

        let hash_str = store.put(&pkg_content)?;

        let pkg_dir = node_modules.join(&node.name);

        // clean any existing directory
        let _ = std::fs::remove_dir_all(&pkg_dir);
        std::fs::create_dir_all(&pkg_dir)?;

        if let Err(e) = extract_tarball(&pkg_content, &pkg_dir) {
            println!("  failed to extract {}: {}", node.name, e);
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
            security: None,
            sbom: None,
        });

        print!("  ✓ {}@{} ({})", node.name, ver_str, hash_str);

        // Analyze package after extraction (non-fatal)
        match analyzer::analyze_package(&pkg_dir) {
            Ok(result) => {
                if !result.findings.is_empty() {
                    let rl = result.risk_level;
                    let first = &result.findings[0];
                    let loc = first.location.as_deref().unwrap_or("");
                    let extra = format!(" ⚠  {} finding(s) ({}) — {} in {}", result.findings.len(), rl, first.description, loc);
                    print!("{}", extra);
                }
            }
            Err(_) => {}
        }

        println!();
    }

    let ts = current_timestamp();

    let lockfile = Lockfile {
        version: 1,
        graph: GraphMeta {
            resolver: "mvs".to_string(),
            generated_at: Some(ts),
            graph_hash: None,
        },
        packages: pkg_entries,
    };

    let lock_content = crate::lockfile::generator::generate(&lockfile);
    let lock_path = cwd.join("ara.lock");
    let mut lock_file = std::fs::File::create(&lock_path)?;
    lock_file.write_all(lock_content.as_bytes())?;
    println!("Lockfile written to ara.lock");

    Ok(())
}

// ── run command ────────────────────────────────────────────────────────────

fn cmd_run(script: &str) -> Result<()> {
    println!("running: {script}");
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(script)
        .status()
        .context("failed to execute script")?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}
