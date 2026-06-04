use anyhow::Result;

use crate::sandbox::executor::Executor;
use crate::sandbox::profiles::{Profile, SandboxConfig};

pub(crate) fn cmd_run(script: &str, profile: &str) -> Result<()> {
    let profile: Profile = profile
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid profile: {e}"))?;
    println!("running: {script} ({profile:?})");
    let config = SandboxConfig::for_profile(profile);
    let executor = Executor::new(config);
    executor.execute(script)?;
    Ok(())
}
