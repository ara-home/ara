use crate::source::SourceError;
use crate::types::{PackageIdentity, Version};
use crate::util::http::HttpClient;

pub struct RegistrySource {
    pub registry_url: String,
}

impl RegistrySource {
    #[must_use]
    pub fn new(registry_url: String) -> Self {
        Self { registry_url }
    }

    pub fn resolve(&self, name: &str) -> Result<String, SourceError> {
        let client = HttpClient::new();
        let url = format!("{}/{name}", self.registry_url);
        let body = client.get(&url).map_err(|e| SourceError::NetworkError(e.to_string()))?;

        let parsed: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| SourceError::PackageNotFound)?;

        let versions = parsed
            .get("versions")
            .and_then(|v| v.as_object())
            .ok_or(SourceError::PackageNotFound)?;

        let mut latest: Option<String> = None;

        for (ver_str, _) in versions {
            if let Ok(ver) = Version::parse(ver_str) {
                match &latest {
                    Some(l) => {
                        if let Ok(current) = Version::parse(l) {
                            if ver > current {
                                latest = Some(ver_str.clone());
                            }
                        }
                    }
                    None => {
                        latest = Some(ver_str.clone());
                    }
                }
            }
        }

        latest.ok_or(SourceError::VersionNotFound)
    }

    pub fn fetch(&self, identity: &PackageIdentity) -> Result<Vec<u8>, SourceError> {
        let client = HttpClient::new();
        let ver_str = format!("{}.{}.{}", identity.version.major, identity.version.minor, identity.version.patch);
        let tarball_url = format!(
            "{}/{name}/-/{name}-{ver_str}.tgz",
            self.registry_url,
            name = identity.name
        );
        let body = client.get(&tarball_url).map_err(|e| SourceError::NetworkError(e.to_string()))?;
        Ok(body)
    }
}
