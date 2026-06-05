use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::sandbox::executor::Executor;
use crate::sandbox::profiles::{Profile, SandboxConfig};

pub(crate) fn cmd_run(script: &str, profile_str: &str) -> Result<()> {
    let profile: Profile = profile_str
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid profile: {e}"))?;

    let cwd = std::env::current_dir().context("failed to get current directory")?;

    // Resolve script name to actual command from manifest
    let command = resolve_script(&cwd, script)?;

    // Build PATH: prepend node_modules/.bin so npm binaries are found
    let bin_dir = cwd.join("node_modules").join(".bin");
    let mut env = HashMap::new();
    if bin_dir.exists() {
        let canonical = bin_dir.canonicalize().unwrap_or(bin_dir);
        let current_path = std::env::var("PATH").unwrap_or_default();
        env.insert(
            "PATH".to_string(),
            format!("{}:{}", canonical.display(), current_path),
        );
    }

    println!("running: {script} -> {command:?}");
    let config = SandboxConfig::for_profile(profile);
    let executor = Executor::new(config);
    executor.execute(&command, Some(env))?;
    Ok(())
}

fn read_manifest(cwd: &Path) -> Result<crate::manifest::types::Manifest> {
    let manifest_path = cwd.join("ara.toml");
    let pkg_json_path = cwd.join("package.json");

    if manifest_path.exists() {
        let content = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let m = crate::manifest::parser::parse(&content).context("failed to parse ara.toml")?;
        return Ok(m);
    }

    if pkg_json_path.exists() {
        let content = std::fs::read_to_string(&pkg_json_path)
            .with_context(|| format!("failed to read {}", pkg_json_path.display()))?;
        let m = crate::manifest::package_json::parse_package_json(&content)
            .context("failed to parse package.json")?;
        return Ok(m);
    }

    Err(anyhow::anyhow!(
        "no ara.toml or package.json found in {}",
        cwd.display()
    ))
}

fn resolve_script(cwd: &Path, name: &str) -> Result<String> {
    let m = read_manifest(cwd)?;
    for script in &m.scripts {
        if script.name == name {
            return Ok(script.command.clone());
        }
    }

    // Fallback: try package.json scripts directly
    let pkg_json_path = cwd.join("package.json");
    if pkg_json_path.exists() {
        let content = std::fs::read_to_string(&pkg_json_path)?;
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(scripts) = json.get("scripts").and_then(|v| v.as_object()) {
                if let Some(cmd) = scripts.get(name).and_then(|v| v.as_str()) {
                    return Ok(cmd.to_string());
                }
            }
        }
    }

    Err(anyhow::anyhow!(
        "script '{name}' not found in ara.toml or package.json"
    ))
}
