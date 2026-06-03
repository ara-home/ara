use std::process::Command;

use crate::source::SourceError;
use crate::types::PackageIdentity;

pub struct GitSource {
    pub url: String,
    pub commit: String,
}

impl GitSource {
    #[must_use]
    pub fn new(url: String, commit: String) -> Self {
        Self { url, commit }
    }

    pub fn resolve(&self, _name: &str) -> Result<String, SourceError> {
        Ok(self.commit.clone())
    }

    pub fn fetch(&self, _identity: &PackageIdentity) -> Result<Vec<u8>, SourceError> {
        let tmp_dir = tempfile::Builder::new()
            .prefix("ara-git-")
            .tempdir()
            .map_err(|e: std::io::Error| SourceError::GitError(e.to_string()))?;
        let tmp_path = tmp_dir.path().join("repo");

        let status = Command::new("git")
            .args(["clone", "--depth", "1", &self.url])
            .arg(&tmp_path)
            .status()
            .map_err(|e| SourceError::GitError(format!("failed to run git: {e}")))?;

        if !status.success() {
            return Err(SourceError::GitError("git clone failed".to_string()));
        }

        let output = Command::new("tar")
            .args(["-C", &tmp_path.to_string_lossy(), "-czf", "-", "."])
            .output()
            .map_err(|e| SourceError::GitError(format!("failed to run tar: {e}")))?;

        if !output.status.success() {
            return Err(SourceError::GitError("tar archive failed".to_string()));
        }

        Ok(output.stdout)
    }
}
