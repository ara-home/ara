use std::process::Command;

use crate::source::SourceError;
use crate::types::PackageIdentity;

pub struct GitSource {
    pub url: String,
    pub commit: String,
}

impl GitSource {
    #[must_use]
    pub const fn new(url: String, commit: String) -> Self {
        Self { url, commit }
    }

    #[allow(clippy::unnecessary_wraps)]
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
            .current_dir("/tmp")
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use std::io::Write;
    use std::process::Command;

    fn create_git_repo(dir: &std::path::Path) {
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .arg(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "test"])
            .current_dir(dir)
            .output()
            .unwrap();
        let mut f = std::fs::File::create(dir.join("file.txt")).unwrap();
        writeln!(f, "hello").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    #[test]
    fn test_fetch_local_git_repo() {
        let repo_dir = tempfile::tempdir().unwrap();
        create_git_repo(repo_dir.path());

        let url = format!("file://{}", repo_dir.path().display());
        let src = GitSource::new(url, "HEAD".to_string());

        let identity = crate::types::PackageIdentity {
            source: crate::types::SourceType::Git,
            name: "test-repo".to_string(),
            version: crate::types::Version::parse("0.1.0").unwrap(),
            content_hash: None,
        };

        let result = src.fetch(&identity).unwrap();
        assert!(result.len() > 0);
        assert_eq!(result[0], 0x1f);
        assert_eq!(result[1], 0x8b);
    }

    #[test]
    fn test_resolve_returns_commit() {
        let src = GitSource::new(
            "https://example.com/repo.git".to_string(),
            "abc123".to_string(),
        );
        assert_eq!(src.resolve("any").unwrap(), "abc123");
    }
}
