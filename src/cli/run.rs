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

    let mut final_manifest: Option<crate::manifest::types::Manifest> = None;

    if pkg_json_path.exists() {
        let content = std::fs::read_to_string(&pkg_json_path)
            .with_context(|| format!("failed to read {}", pkg_json_path.display()))?;
        let m = crate::manifest::package_json::parse_package_json(&content)
            .context("failed to parse package.json")?;
        final_manifest = Some(m);
    }

    if manifest_path.exists() {
        let content = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let m = crate::manifest::parser::parse(&content).context("failed to parse ara.toml")?;

        if let Some(mut fm) = final_manifest {
            fm.security = m.security;
            fm.build = m.build;
            final_manifest = Some(fm);
        } else {
            final_manifest = Some(m);
        }
    }

    if let Some(m) = final_manifest {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_cmd_run_invalid_profile() {
        let res = cmd_run("start", "invalid-profile-name");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("invalid profile"));
    }

    #[test]
    fn test_resolve_script_not_found() {
        let dir = TempDir::new().unwrap();
        let res = resolve_script(dir.path(), "non_existent_script");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("no ara.toml or package.json found"));
    }

    #[test]
    fn test_resolve_script_from_package_json() {
        let dir = TempDir::new().unwrap();
        let pkg_json_path = dir.path().join("package.json");
        let mut f = File::create(pkg_json_path).unwrap();
        writeln!(f, r#"{{"scripts": {{"start": "node index.js"}}}}"#).unwrap();

        let res = resolve_script(dir.path(), "start").unwrap();
        assert_eq!(res, "node index.js");
    }

    #[test]
    fn test_resolve_script_from_ara_toml() {
        let dir = TempDir::new().unwrap();
        let manifest_path = dir.path().join("ara.toml");
        let mut f = File::create(manifest_path).unwrap();
        writeln!(f, r#"
[project]
name = "test"
version = "1.0.0"

[scripts]
test = "echo test"
"#).unwrap();

        let res = resolve_script(dir.path(), "test").unwrap();
        assert_eq!(res, "echo test");
    }
}
