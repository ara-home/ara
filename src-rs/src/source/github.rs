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
        let client = HttpClient::new();
        let ver_str = format!("{}.{}.{}", identity.version.major, identity.version.minor, identity.version.patch);
        let url = format!(
            "https://api.github.com/repos/{repo}/tarball/v{ver_str}",
            repo = self.repo
        );
        let body = client.get(&url).map_err(|e| SourceError::NetworkError(e.to_string()))?;
        Ok(body)
    }
}
