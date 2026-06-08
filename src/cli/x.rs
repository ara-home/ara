use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::sandbox::executor::Executor;
use crate::sandbox::profiles::{Profile, SandboxConfig};

fn quote_arg(arg: &str) -> String {
    if arg
        .chars()
        .all(|c| c.is_alphanumeric() || "-_=/.".contains(c))
        && !arg.is_empty()
    {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

pub(crate) async fn cmd_x(package: &str, args: &[String]) -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("failed to get system time")?
        .as_micros();
    let pid = std::process::id();
    let temp_name = format!("{}-{}", ts, pid);
    let dlx_dir = PathBuf::from(&home)
        .join(".ara")
        .join("dlx")
        .join(temp_name);

    std::fs::create_dir_all(&dlx_dir).context("failed to create temp dlx directory")?;

    let pkg_json = r#"{
  "name": "ara-x-temp",
  "version": "0.0.0",
  "private": true
}"#;
    std::fs::write(dlx_dir.join("package.json"), pkg_json)?;

    let cwd = std::env::current_dir()?;
    std::env::set_current_dir(&dlx_dir)?;

    let install_res = crate::cli::install::cmd_install_specs(
        &[package.to_string()],
        false,
        false,
        false,
        None,
        false,
        false,
        false,
        true,  // non-interactive
        false, // package-lock
    )
    .await;

    std::env::set_current_dir(&cwd)?;

    if let Err(e) = install_res {
        let _ = std::fs::remove_dir_all(&dlx_dir);
        return Err(e);
    }

    let bin_dir = dlx_dir.join("node_modules").join(".bin");

    let pkg_name = package.split('@').next().unwrap_or(package);
    let bare_name = pkg_name.rsplit('/').next().unwrap_or(pkg_name);

    let mut bin_path = bin_dir.join(bare_name);

    if !bin_path.exists() {
        if let Ok(mut entries) = std::fs::read_dir(&bin_dir) {
            // Find the first executable if the exact name doesn't exist
            if let Some(Ok(entry)) = entries.next() {
                bin_path = entry.path();
            }
        }
    }

    if !bin_path.exists() {
        let _ = std::fs::remove_dir_all(&dlx_dir);
        anyhow::bail!(
            "could not find an executable binary for package {}",
            package
        );
    }

    let mut env = HashMap::new();
    let canonical = bin_dir.canonicalize().unwrap_or(bin_dir);
    let current_path = std::env::var("PATH").unwrap_or_default();
    env.insert(
        "PATH".to_string(),
        format!("{}:{}", canonical.display(), current_path),
    );

    let bin_name = bin_path
        .file_name()
        .context("invalid bin path")?
        .to_string_lossy();

    let mut cmd_str = bin_name.to_string();
    for arg in args {
        cmd_str.push(' ');
        cmd_str.push_str(&quote_arg(arg));
    }

    let config = SandboxConfig::for_profile(Profile::Open);
    let executor = Executor::new(config);

    let exec_res = executor.execute(&cmd_str, Some(env));

    let _ = std::fs::remove_dir_all(&dlx_dir);

    exec_res?;
    Ok(())
}
