use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use crate::SourceError;
use ara_types::{Constraint, PackageIdentity, Version};
use ara_util::hash;
use ara_util::http::{HttpClient, HttpError};

const CACHE_TTL: Duration = Duration::from_secs(604800); // 7 days

type DepMap = HashMap<String, String>;

#[derive(Clone)]
pub struct RegistrySource {
    pub registry_url: String,
    client: HttpClient,
}

fn sanitize_cache_name(name: &str) -> String {
    let name = name.replace('/', "_").replace('@', "");
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
}

impl RegistrySource {
    pub fn new(registry_url: String) -> Result<Self, SourceError> {
        let client = HttpClient::new().map_err(|e| SourceError::NetworkError(e.to_string()))?;
        Ok(Self {
            registry_url,
            client,
        })
    }

    /// Pre-warms the HTTP/2 connection to avoid the "Thundering Herd" connection pool problem
    /// where concurrent initial requests cause reqwest to open dozens of TCP sockets
    /// instead of multiplexing over a single HTTP/2 connection.
    pub async fn warmup(&self) {
        let _ = self.client.get(&self.registry_url).await;
    }

    /// Fetch package metadata from the registry, using a local disk cache
    /// for the default npm registry to avoid redundant HTTP requests.
    pub async fn fetch_metadata(&self, name: &str) -> Result<serde_json::Value, SourceError> {
        let use_cache = self.registry_url.contains("registry.npmjs.org");

        if use_cache {
            if let Some(cached) = Self::read_cached_metadata(name) {
                return Ok(cached);
            }
        }

        let url = format!("{}/{name}", self.registry_url);
        let body = self.client.get(&url).await.map_err(|e| match &e {
            HttpError::StatusNotOk(reqwest::StatusCode::NOT_FOUND) => SourceError::PackageNotFound,
            _ => SourceError::NetworkError(e.to_string()),
        })?;

        let parsed: serde_json::Value =
            serde_json::from_slice(&body).map_err(|e| SourceError::ParseError(e.to_string()))?;

        if use_cache {
            Self::write_cached_metadata(name, &parsed);
        }

        Ok(parsed)
    }

    fn cache_dir() -> Option<PathBuf> {
        let home = std::env::var("HOME").ok()?;
        Some(
            PathBuf::from(home)
                .join(".ara")
                .join("cache")
                .join("metadata"),
        )
    }

    fn cache_path(name: &str) -> Option<PathBuf> {
        let dir = Self::cache_dir()?;
        let safe_name = sanitize_cache_name(name);
        Some(dir.join(format!("{safe_name}.json")))
    }

    fn cache_integrity_path(name: &str) -> Option<PathBuf> {
        let dir = Self::cache_dir()?;
        let safe_name = sanitize_cache_name(name);
        Some(dir.join(format!("{safe_name}.json.sha256")))
    }

    fn read_cached_metadata(name: &str) -> Option<serde_json::Value> {
        let path = Self::cache_path(name)?;
        if !path.exists() {
            return None;
        }
        let metadata = std::fs::metadata(&path).ok()?;
        if let Ok(modified) = metadata.modified() {
            if let Ok(elapsed) = modified.elapsed() {
                if elapsed > CACHE_TTL {
                    let _ = std::fs::remove_file(&path);
                    return None;
                }
            }
        }
        let file = std::fs::File::open(&path).ok()?;
        let reader = std::io::BufReader::new(file);
        let parsed: serde_json::Value = serde_json::from_reader(reader).ok()?;

        // Verify integrity if sidecar file exists
        let integrity_ok = Self::cache_integrity_path(name).and_then(|hash_path| {
            let expected = std::fs::read_to_string(&hash_path).ok()?;
            let content = serde_json::to_string(&parsed).ok()?;
            let actual = hash::hex_encode(&hash::compute(content.as_bytes()));
            if actual == expected.trim() {
                Some(())
            } else {
                None
            }
        });
        if integrity_ok.is_none() {
            // No integrity file or mismatch — invalidate cache
            let _ = std::fs::remove_file(&path);
            return None;
        }

        // Support legacy format: unwrap {"body": metadata} wrapper
        if parsed.get("body").and_then(|b| b.get("versions")).is_some() {
            parsed.get("body").cloned()
        } else {
            Some(parsed)
        }
    }

    fn write_cached_metadata(name: &str, body: &serde_json::Value) {
        let path = match Self::cache_path(name) {
            Some(p) => p,
            None => return,
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string(body) {
            let _ = std::fs::write(&path, &content);
            // Write integrity sidecar
            if let Some(hash_path) = Self::cache_integrity_path(name) {
                let h = hash::hex_encode(&hash::compute(content.as_bytes()));
                let _ = std::fs::write(&hash_path, &h);
            }
        }
    }

    pub async fn resolve(&self, name: &str) -> Result<String, SourceError> {
        let parsed = self.fetch_metadata(name).await?;

        // Prefer the `latest` dist-tag when present (matches npm behavior)
        if let Some(latest_tag) = parsed
            .get("dist-tags")
            .and_then(|t| t.get("latest"))
            .and_then(|v| v.as_str())
        {
            return Ok(latest_tag.to_string());
        }

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

    /// Resolve the best version matching a constraint string (e.g., `^0.5.15`).
    /// Returns the highest published version that satisfies the constraint.
    pub async fn resolve_matching(
        &self,
        name: &str,
        constraint_str: &str,
    ) -> Result<String, SourceError> {
        let parsed = self.fetch_metadata(name).await?;

        let constraint =
            Constraint::parse(constraint_str).map_err(|_| SourceError::VersionNotFound)?;

        let versions = parsed
            .get("versions")
            .and_then(|v| v.as_object())
            .ok_or(SourceError::PackageNotFound)?;

        let mut best: Option<Version> = None;
        for (ver_str, _) in versions {
            if let Ok(ver) = Version::parse(ver_str) {
                if constraint.satisfied_by(&ver) {
                    match &best {
                        Some(current) if ver > *current => best = Some(ver),
                        None => best = Some(ver),
                        _ => {}
                    }
                }
            }
        }

        best.map(|v| v.to_string())
            .ok_or(SourceError::VersionNotFound)
    }

    pub async fn fetch(&self, identity: &PackageIdentity) -> Result<Vec<u8>, SourceError> {
        let ver_str = identity.version.to_string();
        // For scoped packages (@scope/name), the tarball filename uses only the bare name
        let bare_name = identity.name.rsplit('/').next().unwrap_or(&identity.name);
        let tarball_url = format!(
            "{}/{name}/-/{bare_name}-{ver_str}.tgz",
            self.registry_url,
            name = identity.name
        );
        let body = self.client.get(&tarball_url).await.map_err(|e| match &e {
            HttpError::StatusNotOk(reqwest::StatusCode::NOT_FOUND) => SourceError::VersionNotFound,
            _ => SourceError::NetworkError(e.to_string()),
        })?;

        if let Some(ref expected) = identity.content_hash {
            if !ara_util::hash::verify_integrity(&body, expected) {
                let actual = ara_util::hash::format_sha256(&body);
                // Also try shasum (plain hex) comparison
                let expected_hex = expected.trim();
                if expected_hex.len() == 64
                    && ara_util::hash::hex_encode(&ara_util::hash::compute(&body)) == expected_hex
                {
                    return Ok(body);
                }
                return Err(SourceError::IntegrityMismatch {
                    expected: expected.clone(),
                    actual,
                });
            }
        }

        Ok(body)
    }

    /// Resolve the exact version matching a constraint, then return that
    /// version's dependency declarations from the registry metadata.
    /// Returns (exact_version, dependencies, peer_dependencies, optional_dependencies).
    pub async fn resolve_and_get_deps(
        &self,
        name: &str,
        constraint_str: &str,
    ) -> Result<(String, DepMap, DepMap, DepMap), SourceError> {
        let parsed = self.fetch_metadata(name).await?;

        let constraint =
            Constraint::parse(constraint_str).map_err(|_| SourceError::VersionNotFound)?;

        let versions = parsed
            .get("versions")
            .and_then(|v| v.as_object())
            .ok_or(SourceError::PackageNotFound)?;

        // Find best matching version
        let mut best: Option<&serde_json::Value> = None;
        let mut best_ver: Option<String> = None;
        for (ver_str, ver_data) in versions {
            if let Ok(ver) = Version::parse(ver_str) {
                if constraint.satisfied_by(&ver) {
                    match &best_ver {
                        Some(ref current) => {
                            if let Ok(current_ver) = Version::parse(current) {
                                if ver > current_ver {
                                    best = Some(ver_data);
                                    best_ver = Some(ver_str.clone());
                                }
                            }
                        }
                        None => {
                            best = Some(ver_data);
                            best_ver = Some(ver_str.clone());
                        }
                    }
                }
            }
        }

        let ver_str = best_ver.ok_or(SourceError::VersionNotFound)?;
        let ver_data = best.ok_or(SourceError::VersionNotFound)?;

        let extract_deps = |key: &str| -> HashMap<String, String> {
            ver_data
                .get(key)
                .and_then(|v| v.as_object())
                .map(|map| {
                    map.iter()
                        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                        .collect()
                })
                .unwrap_or_default()
        };

        Ok((
            ver_str,
            extract_deps("dependencies"),
            extract_deps("peerDependencies"),
            extract_deps("optionalDependencies"),
        ))
    }

    /// Given an exact package name and version, read its dependency
    /// declarations from the registry metadata (no resolution needed).
    pub async fn get_deps_for_version(
        &self,
        name: &str,
        version_str: &str,
    ) -> Result<(DepMap, DepMap, DepMap), SourceError> {
        let parsed = self.fetch_metadata(name).await?;

        let ver_data = parsed
            .get("versions")
            .and_then(|v| v.as_object())
            .and_then(|v| v.get(version_str))
            .ok_or(SourceError::VersionNotFound)?;

        let extract_deps = |key: &str| -> HashMap<String, String> {
            ver_data
                .get(key)
                .and_then(|v| v.as_object())
                .map(|map| {
                    map.iter()
                        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                        .collect()
                })
                .unwrap_or_default()
        };

        Ok((
            extract_deps("dependencies"),
            extract_deps("peerDependencies"),
            extract_deps("optionalDependencies"),
        ))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[tokio::test]
    async fn test_resolve_finds_latest_version() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        let body = serde_json::json!({
            "versions": {
                "1.0.0": {},
                "2.0.0": {},
                "1.5.0": {}
            }
        });

        let mock = server
            .mock("GET", "/zod")
            .with_status(200)
            .with_body(body.to_string())
            .create_async()
            .await;

        let src = RegistrySource::new(url.to_string()).unwrap();
        let version = src.resolve("zod").await.unwrap();
        assert_eq!(version, "2.0.0");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_resolve_package_not_found_404() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        let mock = server
            .mock("GET", "/missing")
            .with_status(404)
            .create_async()
            .await;

        let src = RegistrySource::new(url.to_string()).unwrap();
        let err = src.resolve("missing").await.unwrap_err();
        assert!(matches!(err, SourceError::PackageNotFound));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_resolve_invalid_json() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        let _mock = server
            .mock("GET", "/bad")
            .with_status(200)
            .with_body("this is not json")
            .create_async()
            .await;

        let src = RegistrySource::new(url.to_string()).unwrap();
        let err = src.resolve("bad").await.unwrap_err();
        assert!(matches!(err, SourceError::ParseError(_)));
    }

    #[tokio::test]
    async fn test_resolve_no_versions() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        let body = serde_json::json!({ "versions": {} });

        let _mock = server
            .mock("GET", "/empty")
            .with_status(200)
            .with_body(body.to_string())
            .create_async()
            .await;

        let src = RegistrySource::new(url.to_string()).unwrap();
        let err = src.resolve("empty").await.unwrap_err();
        assert!(matches!(err, SourceError::VersionNotFound));
    }

    #[tokio::test]
    async fn test_resolve_prefers_dist_tags_latest() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        // dist-tags.latest points to 2.0.0 even though 3.0.0-canary exists
        let body = serde_json::json!({
            "dist-tags": { "latest": "2.0.0" },
            "versions": {
                "2.0.0": {},
                "3.0.0-canary.1": {}
            }
        });

        let _mock = server
            .mock("GET", "/pkg")
            .with_status(200)
            .with_body(body.to_string())
            .create_async()
            .await;

        let src = RegistrySource::new(url.to_string()).unwrap();
        let version = src.resolve("pkg").await.unwrap();
        assert_eq!(version, "2.0.0");
    }

    #[tokio::test]
    async fn test_resolve_fallback_highest_semver_no_dist_tags() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        let body = serde_json::json!({
            "versions": {
                "1.0.0": {},
                "3.0.0": {},
                "2.0.0": {}
            }
        });

        let _mock = server
            .mock("GET", "/naked")
            .with_status(200)
            .with_body(body.to_string())
            .create_async()
            .await;

        let src = RegistrySource::new(url.to_string()).unwrap();
        let version = src.resolve("naked").await.unwrap();
        assert_eq!(version, "3.0.0");
    }

    #[tokio::test]
    async fn test_fetch_tarball_with_prerelease() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        let _mock = server
            .mock("GET", "/next/-/next-16.3.0-canary.41.tgz")
            .with_status(200)
            .with_body(b"fake-next-tarball")
            .create_async()
            .await;

        let src = RegistrySource::new(url.to_string()).unwrap();
        let identity = ara_types::PackageIdentity {
            source: ara_types::SourceType::Npm,
            name: "next".to_string(),
            version: ara_types::Version::parse("16.3.0-canary.41").unwrap(),
            content_hash: None,
            requested_ref: None,
        };
        let result = src.fetch(&identity).await.unwrap();
        assert_eq!(result, b"fake-next-tarball");
    }

    #[tokio::test]
    async fn test_resolve_scoped_package() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        let body = serde_json::json!({
            "dist-tags": { "latest": "2.0.13" },
            "versions": {
                "2.0.13": {},
                "1.0.0": {}
            }
        });

        let _mock = server
            .mock("GET", "/@types/mdx")
            .with_status(200)
            .with_body(body.to_string())
            .create_async()
            .await;

        let src = RegistrySource::new(url.to_string()).unwrap();
        let version = src.resolve("@types/mdx").await.unwrap();
        assert_eq!(version, "2.0.13");
    }

    #[tokio::test]
    async fn test_fetch_scoped_package_tarball() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();

        // Scoped tarball URL uses bare name: mdx-2.0.13.tgz (not @types/mdx-2.0.13.tgz)
        let _mock = server
            .mock("GET", "/@types/mdx/-/mdx-2.0.13.tgz")
            .with_status(200)
            .with_body(b"fake-mdx-tarball")
            .create_async()
            .await;

        let src = RegistrySource::new(url.to_string()).unwrap();
        let identity = ara_types::PackageIdentity {
            source: ara_types::SourceType::Npm,
            name: "@types/mdx".to_string(),
            version: ara_types::Version::parse("2.0.13").unwrap(),
            content_hash: None,
            requested_ref: None,
        };
        let result = src.fetch(&identity).await.unwrap();
        assert_eq!(result, b"fake-mdx-tarball");
    }

    #[tokio::test]
    async fn test_fetch_tarball() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        let tarball = b"fake-tarball-content";

        let mock = server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"^/zod/-/zod-.*$".to_string()),
            )
            .with_status(200)
            .with_body(tarball)
            .create_async()
            .await;

        let src = RegistrySource::new(url.to_string()).unwrap();
        let identity = ara_types::PackageIdentity {
            source: ara_types::SourceType::Npm,
            name: "zod".to_string(),
            version: ara_types::Version::parse("3.23.8").unwrap(),
            content_hash: None,
            requested_ref: None,
        };
        let result = src.fetch(&identity).await.unwrap();
        assert_eq!(result, tarball);
        mock.assert_async().await;
    }
}
