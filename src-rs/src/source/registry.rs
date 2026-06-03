use crate::source::SourceError;
use crate::types::{PackageIdentity, Version};
use crate::util::http::HttpClient;

pub struct RegistrySource {
    pub registry_url: String,
}

impl RegistrySource {
    #[must_use]
    pub const fn new(registry_url: String) -> Self {
        Self { registry_url }
    }

    pub fn resolve(&self, name: &str) -> Result<String, SourceError> {
        let client = HttpClient::new().map_err(|e| SourceError::NetworkError(e.to_string()))?;
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
        let client = HttpClient::new().map_err(|e| SourceError::NetworkError(e.to_string()))?;
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_resolve_finds_latest_version() {
        let mut server = mockito::Server::new();
        let url = server.url();
        let body = serde_json::json!({
            "versions": {
                "1.0.0": {},
                "2.0.0": {},
                "1.5.0": {}
            }
        });

        let mock = server.mock("GET", "/zod")
            .with_status(200)
            .with_body(body.to_string())
            .create();

        let src = RegistrySource::new(format!("{url}"));
        let version = src.resolve("zod").unwrap();
        assert_eq!(version, "2.0.0");
        mock.assert();
    }

    #[test]
    fn test_resolve_package_not_found_404() {
        let mut server = mockito::Server::new();
        let url = server.url();
        let mock = server.mock("GET", "/missing")
            .with_status(404)
            .create();

        let src = RegistrySource::new(format!("{url}"));
        let err = src.resolve("missing").unwrap_err();
        assert!(matches!(err, SourceError::NetworkError(_)));
        mock.assert();
    }

    #[test]
    fn test_resolve_invalid_json() {
        let mut server = mockito::Server::new();
        let url = server.url();
        let _mock = server.mock("GET", "/bad")
            .with_status(200)
            .with_body("this is not json")
            .create();

        let src = RegistrySource::new(format!("{url}"));
        let err = src.resolve("bad").unwrap_err();
        assert!(matches!(err, SourceError::PackageNotFound));
    }

    #[test]
    fn test_resolve_no_versions() {
        let mut server = mockito::Server::new();
        let url = server.url();
        let body = serde_json::json!({ "versions": {} });

        let _mock = server.mock("GET", "/empty")
            .with_status(200)
            .with_body(body.to_string())
            .create();

        let src = RegistrySource::new(format!("{url}"));
        let err = src.resolve("empty").unwrap_err();
        assert!(matches!(err, SourceError::VersionNotFound));
    }

    #[test]
    fn test_fetch_tarball() {
        let mut server = mockito::Server::new();
        let url = server.url();
        let tarball = b"fake-tarball-content";

        let mock = server.mock("GET", mockito::Matcher::Regex(r"^/zod/-/zod-.*$".to_string()))
            .with_status(200)
            .with_body(tarball)
            .create();

        let src = RegistrySource::new(format!("{url}"));
        let identity = crate::types::PackageIdentity {
            source: crate::types::SourceType::Npm,
            name: "zod".to_string(),
            version: crate::types::Version::parse("3.23.8").unwrap(),
            content_hash: None,
        };
        let result = src.fetch(&identity).unwrap();
        assert_eq!(result, tarball);
        mock.assert();
    }
}
