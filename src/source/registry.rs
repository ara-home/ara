use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use crate::source::SourceError;
use crate::types::{Constraint, PackageIdentity, Version};
use crate::util::http::HttpClient;

const CACHE_TTL: Duration = Duration::from_secs(604800); // 7 days

type DepMap = HashMap<String, String>;

pub struct RegistrySource {
    pub registry_url: String,
    client: HttpClient,
}

impl RegistrySource {
    pub fn new(registry_url: String) -> Result<Self, SourceError> {
        let client = HttpClient::new().map_err(|e| SourceError::NetworkError(e.to_string()))?;
        Ok(Self {
            registry_url,
            client,
        })
    }

    /// Fetch package metadata from the registry, using a local disk cache
    /// for the default npm registry to avoid redundant HTTP requests.
    pub(crate) fn fetch_metadata(&self, name: &str) -> Result<serde_json::Value, SourceError> {
        let use_cache = self.registry_url.contains("registry.npmjs.org");

        if use_cache {
            if let Some(cached) = Self::read_cached_metadata(name) {
                return Ok(cached);
            }
        }

        let url = format!("{}/{name}", self.registry_url);
        let body = self
            .client
            .get(&url)
            .map_err(|e| SourceError::NetworkError(e.to_string()))?;

        let parsed: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| SourceError::PackageNotFound)?;

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
        let safe_name = name.replace('/', "_").replace('@', "");
        Some(dir.join(format!("{safe_name}.json")))
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
        let content = std::fs::read_to_string(path).ok()?;
        let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
        parsed.get("body").cloned()
    }

    fn write_cached_metadata(name: &str, body: &serde_json::Value) {
        let path = match Self::cache_path(name) {
            Some(p) => p,
            None => return,
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let cache = serde_json::json!({ "body": body });
        if let Ok(content) = serde_json::to_string(&cache) {
            let _ = std::fs::write(&path, content);
        }
    }

    pub fn resolve(&self, name: &str) -> Result<String, SourceError> {
        let parsed = self.fetch_metadata(name)?;

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
    pub fn resolve_matching(
        &self,
        name: &str,
        constraint_str: &str,
    ) -> Result<String, SourceError> {
        let parsed = self.fetch_metadata(name)?;

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

    pub fn fetch(&self, identity: &PackageIdentity) -> Result<Vec<u8>, SourceError> {
        let ver_str = identity.version.to_string();
        // For scoped packages (@scope/name), the tarball filename uses only the bare name
        let bare_name = identity.name.rsplit('/').next().unwrap_or(&identity.name);
        let tarball_url = format!(
            "{}/{name}/-/{bare_name}-{ver_str}.tgz",
            self.registry_url,
            name = identity.name
        );
        let body = self
            .client
            .get(&tarball_url)
            .map_err(|e| SourceError::NetworkError(e.to_string()))?;
        Ok(body)
    }

    /// Resolve the exact version matching a constraint, then return that
    /// version's dependency declarations from the registry metadata.
    /// Returns (exact_version, dependencies, peer_dependencies, optional_dependencies).
    pub fn resolve_and_get_deps(
        &self,
        name: &str,
        constraint_str: &str,
    ) -> Result<(String, DepMap, DepMap, DepMap), SourceError> {
        let parsed = self.fetch_metadata(name)?;

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
    pub fn get_deps_for_version(
        &self,
        name: &str,
        version_str: &str,
    ) -> Result<(DepMap, DepMap, DepMap), SourceError> {
        let parsed = self.fetch_metadata(name)?;

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

    /// Extract dependency declarations from pre-fetched metadata for an exact version.
    pub(crate) fn get_deps_for_version_from_meta(
        &self,
        metadata: &serde_json::Value,
        version_str: &str,
    ) -> Result<(DepMap, DepMap, DepMap), SourceError> {
        let ver_data = metadata
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

    /// Resolve the best matching version + extract deps from pre-fetched metadata.
    pub(crate) fn resolve_and_get_deps_from_meta(
        &self,
        metadata: &serde_json::Value,
        constraint_str: &str,
    ) -> Result<(String, DepMap, DepMap, DepMap), SourceError> {
        let constraint =
            Constraint::parse(constraint_str).map_err(|_| SourceError::VersionNotFound)?;

        let versions = metadata
            .get("versions")
            .and_then(|v| v.as_object())
            .ok_or(SourceError::PackageNotFound)?;

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

        let mock = server
            .mock("GET", "/zod")
            .with_status(200)
            .with_body(body.to_string())
            .create();

        let src = RegistrySource::new(format!("{url}")).unwrap();
        let version = src.resolve("zod").unwrap();
        assert_eq!(version, "2.0.0");
        mock.assert();
    }

    #[test]
    fn test_resolve_package_not_found_404() {
        let mut server = mockito::Server::new();
        let url = server.url();
        let mock = server.mock("GET", "/missing").with_status(404).create();

        let src = RegistrySource::new(format!("{url}")).unwrap();
        let err = src.resolve("missing").unwrap_err();
        assert!(matches!(err, SourceError::NetworkError(_)));
        mock.assert();
    }

    #[test]
    fn test_resolve_invalid_json() {
        let mut server = mockito::Server::new();
        let url = server.url();
        let _mock = server
            .mock("GET", "/bad")
            .with_status(200)
            .with_body("this is not json")
            .create();

        let src = RegistrySource::new(format!("{url}")).unwrap();
        let err = src.resolve("bad").unwrap_err();
        assert!(matches!(err, SourceError::PackageNotFound));
    }

    #[test]
    fn test_resolve_no_versions() {
        let mut server = mockito::Server::new();
        let url = server.url();
        let body = serde_json::json!({ "versions": {} });

        let _mock = server
            .mock("GET", "/empty")
            .with_status(200)
            .with_body(body.to_string())
            .create();

        let src = RegistrySource::new(format!("{url}")).unwrap();
        let err = src.resolve("empty").unwrap_err();
        assert!(matches!(err, SourceError::VersionNotFound));
    }

    #[test]
    fn test_resolve_prefers_dist_tags_latest() {
        let mut server = mockito::Server::new();
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
            .create();

        let src = RegistrySource::new(format!("{url}")).unwrap();
        let version = src.resolve("pkg").unwrap();
        assert_eq!(version, "2.0.0");
    }

    #[test]
    fn test_resolve_fallback_highest_semver_no_dist_tags() {
        let mut server = mockito::Server::new();
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
            .create();

        let src = RegistrySource::new(format!("{url}")).unwrap();
        let version = src.resolve("naked").unwrap();
        assert_eq!(version, "3.0.0");
    }

    #[test]
    fn test_fetch_tarball_with_prerelease() {
        let mut server = mockito::Server::new();
        let url = server.url();

        let _mock = server
            .mock("GET", "/next/-/next-16.3.0-canary.41.tgz")
            .with_status(200)
            .with_body(b"fake-next-tarball")
            .create();

        let src = RegistrySource::new(format!("{url}")).unwrap();
        let identity = crate::types::PackageIdentity {
            source: crate::types::SourceType::Npm,
            name: "next".to_string(),
            version: crate::types::Version::parse("16.3.0-canary.41").unwrap(),
            content_hash: None,
            requested_ref: None,
        };
        let result = src.fetch(&identity).unwrap();
        assert_eq!(result, b"fake-next-tarball");
    }

    #[test]
    fn test_resolve_scoped_package() {
        let mut server = mockito::Server::new();
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
            .create();

        let src = RegistrySource::new(format!("{url}")).unwrap();
        let version = src.resolve("@types/mdx").unwrap();
        assert_eq!(version, "2.0.13");
    }

    #[test]
    fn test_fetch_scoped_package_tarball() {
        let mut server = mockito::Server::new();
        let url = server.url();

        // Scoped tarball URL uses bare name: mdx-2.0.13.tgz (not @types/mdx-2.0.13.tgz)
        let _mock = server
            .mock("GET", "/@types/mdx/-/mdx-2.0.13.tgz")
            .with_status(200)
            .with_body(b"fake-mdx-tarball")
            .create();

        let src = RegistrySource::new(format!("{url}")).unwrap();
        let identity = crate::types::PackageIdentity {
            source: crate::types::SourceType::Npm,
            name: "@types/mdx".to_string(),
            version: crate::types::Version::parse("2.0.13").unwrap(),
            content_hash: None,
            requested_ref: None,
        };
        let result = src.fetch(&identity).unwrap();
        assert_eq!(result, b"fake-mdx-tarball");
    }

    #[test]
    fn test_fetch_tarball() {
        let mut server = mockito::Server::new();
        let url = server.url();
        let tarball = b"fake-tarball-content";

        let mock = server
            .mock(
                "GET",
                mockito::Matcher::Regex(r"^/zod/-/zod-.*$".to_string()),
            )
            .with_status(200)
            .with_body(tarball)
            .create();

        let src = RegistrySource::new(format!("{url}")).unwrap();
        let identity = crate::types::PackageIdentity {
            source: crate::types::SourceType::Npm,
            name: "zod".to_string(),
            version: crate::types::Version::parse("3.23.8").unwrap(),
            content_hash: None,
            requested_ref: None,
        };
        let result = src.fetch(&identity).unwrap();
        assert_eq!(result, tarball);
        mock.assert();
    }
}
