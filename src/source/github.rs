use crate::source::SourceError;
use crate::types::PackageIdentity;
use crate::util::http::HttpClient;

pub struct GithubSource {
    pub repo: String,
}

impl GithubSource {
    #[must_use]
    pub const fn new(repo: String) -> Self {
        Self { repo }
    }

    #[allow(clippy::unnecessary_wraps)]
    pub async fn resolve(&self, _name: &str) -> Result<String, SourceError> {
        Ok(self.repo.clone())
    }

    pub async fn fetch(&self, identity: &PackageIdentity) -> Result<Vec<u8>, SourceError> {
        let client = HttpClient::new().map_err(|e| SourceError::NetworkError(e.to_string()))?;
        let ref_str = identity.requested_ref.as_deref().unwrap_or("HEAD");
        let url = format!(
            "https://api.github.com/repos/{repo}/tarball/{ref_str}",
            repo = self.repo
        );
        let body = client
            .get(&url)
            .await
            .map_err(|e| SourceError::NetworkError(e.to_string()))?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[tokio::test]
    async fn test_resolve_returns_repo() {
        let src = GithubSource::new("owner/repo".to_string());
        assert_eq!(src.resolve("any").await.unwrap(), "owner/repo");
    }

    #[test]
    fn test_github_source_new() {
        let src = GithubSource::new("user/project".to_string());
        assert_eq!(src.repo, "user/project");
    }
}
