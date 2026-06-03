use crate::source::SourceError;
use crate::types::PackageIdentity;
use crate::util::http::HttpClient;

pub struct GithubSource {
    pub repo: String,
}

impl GithubSource {
    #[must_use]
    pub fn new(repo: String) -> Self {
        Self { repo }
    }

    pub fn resolve(&self, _name: &str) -> Result<String, SourceError> {
        Ok(self.repo.clone())
    }

    pub fn fetch(&self, identity: &PackageIdentity) -> Result<Vec<u8>, SourceError> {
        let client = HttpClient::new().map_err(|e| SourceError::NetworkError(e.to_string()))?;
        let ver_str = format!("{}.{}.{}", identity.version.major, identity.version.minor, identity.version.patch);
        let url = format!(
            "https://api.github.com/repos/{repo}/tarball/v{ver_str}",
            repo = self.repo
        );
        let body = client.get(&url).map_err(|e| SourceError::NetworkError(e.to_string()))?;
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_resolve_returns_repo() {
        let src = GithubSource::new("owner/repo".to_string());
        assert_eq!(src.resolve("any").unwrap(), "owner/repo");
    }

    #[test]
    fn test_github_source_new() {
        let src = GithubSource::new("user/project".to_string());
        assert_eq!(src.repo, "user/project");
    }
}
