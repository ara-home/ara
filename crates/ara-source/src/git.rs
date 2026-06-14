use std::process::Command;

use crate::SourceError;
use ara_types::PackageIdentity;

const ALLOWED_GIT_SCHEMES: &[&str] = &["https://", "ssh://", "git://", "git+https://"];

fn validate_git_url(url: &str) -> Result<(), SourceError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(SourceError::GitError("empty git URL".to_string()));
    }
    let lower = trimmed.to_lowercase();
    // Reject file:// and ext:// explicitly
    if lower.starts_with("file://") {
        return Err(SourceError::GitError(
            "file:// git URLs are not allowed for security reasons".to_string(),
        ));
    }
    if lower.starts_with("ext:") {
        return Err(SourceError::GitError(
            "ext: git protocol is not allowed for security reasons".to_string(),
        ));
    }
    // If URL has a scheme, only allow known secure ones
    if trimmed.contains("://") {
        let allowed = ALLOWED_GIT_SCHEMES.iter().any(|s| lower.starts_with(s));
        if !allowed {
            return Err(SourceError::GitError(format!(
                "disallowed git URL scheme: {}. allowed schemes: https://, ssh://, git://",
                trimmed.split("://").next().unwrap_or(trimmed)
            )));
        }
    }
    // No scheme or allowed scheme — acceptable (local paths have no scheme)
    Ok(())
}

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
    pub async fn resolve(&self, _name: &str) -> Result<String, SourceError> {
        Ok(self.commit.clone())
    }

    pub async fn fetch(&self, identity: &PackageIdentity) -> Result<Vec<u8>, SourceError> {
        validate_git_url(&self.url)?;

        let tmp_dir = tempfile::Builder::new()
            .prefix("ara-git-")
            .tempdir()
            .map_err(|e: std::io::Error| SourceError::GitError(e.to_string()))?;
        let tmp_path = tmp_dir.path().join("repo");

        let ref_str = identity.requested_ref.as_deref().unwrap_or(&self.commit);

        if ref_str == "HEAD" {
            // Shallow clone the default branch
            let status = Command::new("git")
                .args([
                    "clone",
                    "--depth",
                    "1",
                    &self.url,
                    &tmp_path.to_string_lossy(),
                ])
                .status()
                .map_err(|e| SourceError::GitError(format!("failed to run git: {e}")))?;
            if !status.success() {
                return Err(SourceError::GitError("git clone failed".to_string()));
            }
        } else {
            // Fetch a specific ref (commit, tag, or branch) with depth 1
            let init = Command::new("git")
                .args(["init"])
                .arg(&tmp_path)
                .status()
                .map_err(|e| SourceError::GitError(format!("failed to run git init: {e}")))?;
            if !init.success() {
                return Err(SourceError::GitError("git init failed".to_string()));
            }
            let add_remote = Command::new("git")
                .args(["remote", "add", "origin", &self.url])
                .current_dir(&tmp_path)
                .status()
                .map_err(|e| SourceError::GitError(format!("failed to add remote: {e}")))?;
            if !add_remote.success() {
                return Err(SourceError::GitError("git remote add failed".to_string()));
            }
            let fetch = Command::new("git")
                .args(["fetch", "--depth", "1", "origin", ref_str])
                .current_dir(&tmp_path)
                .status()
                .map_err(|e| SourceError::GitError(format!("failed to fetch: {e}")))?;
            if !fetch.success() {
                return Err(SourceError::GitError(
                    "git fetch ref failed — ref may not exist".to_string(),
                ));
            }
            let checkout = Command::new("git")
                .args(["checkout", "FETCH_HEAD"])
                .current_dir(&tmp_path)
                .status()
                .map_err(|e| SourceError::GitError(format!("failed to checkout: {e}")))?;
            if !checkout.success() {
                return Err(SourceError::GitError("git checkout ref failed".to_string()));
            }
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

    #[tokio::test]
    async fn test_fetch_local_git_repo() {
        let repo_dir = tempfile::tempdir().unwrap();
        create_git_repo(repo_dir.path());

        // Use the path directly (git accepts local paths without file://)
        let url = repo_dir.path().to_string_lossy().to_string();
        let src = GitSource::new(url, "HEAD".to_string());

        let identity = ara_types::PackageIdentity {
            source: ara_types::SourceType::Git,
            name: "test-repo".to_string(),
            version: ara_types::Version::parse("0.1.0").unwrap(),
            content_hash: None,
            requested_ref: None,
        };

        let result = src.fetch(&identity).await.unwrap();
        assert!(!result.is_empty());
        assert_eq!(result[0], 0x1f);
        assert_eq!(result[1], 0x8b);
    }

    #[tokio::test]
    async fn test_resolve_returns_commit() {
        let src = GitSource::new(
            "https://example.com/repo.git".to_string(),
            "abc123".to_string(),
        );
        assert_eq!(src.resolve("any").await.unwrap(), "abc123");
    }

    #[tokio::test]
    async fn test_validate_rejects_file_url() {
        let src = GitSource::new("file:///etc/passwd".to_string(), "HEAD".to_string());
        let identity = ara_types::PackageIdentity {
            source: ara_types::SourceType::Git,
            name: "test".to_string(),
            version: ara_types::Version::parse("0.1.0").unwrap(),
            content_hash: None,
            requested_ref: None,
        };
        let result = src.fetch(&identity).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not allowed"),
            "expected 'not allowed' error, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_validate_rejects_empty_url() {
        let src = GitSource::new("".to_string(), "HEAD".to_string());
        let identity = ara_types::PackageIdentity {
            source: ara_types::SourceType::Git,
            name: "test".to_string(),
            version: ara_types::Version::parse("0.1.0").unwrap(),
            content_hash: None,
            requested_ref: None,
        };
        let result = src.fetch(&identity).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_validate_allows_https() {
        assert!(validate_git_url("https://github.com/user/repo.git").is_ok());
        assert!(validate_git_url("ssh://git@github.com/user/repo.git").is_ok());
        assert!(validate_git_url("git://github.com/user/repo.git").is_ok());
    }
}
