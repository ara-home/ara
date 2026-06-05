use crate::source::SourceError;
use crate::types::PackageIdentity;
use crate::util::http::HttpClient;

pub struct TarballSource {
    pub url: String,
}

impl TarballSource {
    #[must_use]
    pub const fn new(url: String) -> Self {
        Self { url }
    }

    /// Tarball source doesn't resolve — the URL is the identity.
    /// Returns the URL itself as the "version" for consistency.
    #[allow(clippy::unnecessary_wraps)]
    pub fn resolve(&self, _name: &str) -> Result<String, SourceError> {
        Ok(self.url.clone())
    }

    pub fn fetch(&self, _identity: &PackageIdentity) -> Result<Vec<u8>, SourceError> {
        let client = HttpClient::new().map_err(|e| SourceError::NetworkError(e.to_string()))?;
        let body = client
            .get(&self.url)
            .map_err(|e| SourceError::NetworkError(e.to_string()))?;
        Ok(body)
    }
}

/// Extract the package name and version from a gzipped tarball's `package.json`.
///
/// If the tarball has no `package.json` or it's malformed, returns an error.
/// Prefers `package/package.json` (npm convention) but falls back to any
/// top-level `package.json` if the nested one is absent.
pub fn identity_from_tarball(tarball: &[u8]) -> Result<(String, String), SourceError> {
    use std::io::Read;

    let decoder = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(decoder);

    let mut pkg_json_bytes: Option<Vec<u8>> = None;
    let mut has_nested = false;

    for entry in archive
        .entries()
        .map_err(|e| SourceError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?
    {
        let mut entry = entry.map_err(|e| {
            SourceError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        let path = entry.path().map_err(|e| {
            SourceError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;

        let path_str = path.to_string_lossy();
        let is_nested = path_str == "package/package.json" || path_str == "./package/package.json";
        let is_top = path_str == "package.json" || path_str == "./package.json";

        if is_nested || (is_top && !has_nested) {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).map_err(|e| {
                SourceError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
            })?;
            if is_nested {
                pkg_json_bytes = Some(buf);
                has_nested = true;
            } else if !has_nested {
                pkg_json_bytes = Some(buf);
            }
        }
    }

    let bytes = pkg_json_bytes.ok_or(SourceError::PackageNotFound)?;

    let parsed: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| SourceError::PackageNotFound)?;

    let name = parsed
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or(SourceError::PackageNotFound)?;

    let version = parsed
        .get("version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "0.0.0".to_string());

    Ok((name, version))
}

/// Derive a package name from a tarball URL filename.
///
/// Returns the filename without the archive extension. The caller is
/// responsible for any further version-stripping logic.
///
/// Examples:
/// - `https://example.com/pkg-1.2.3.tgz` → `pkg-1.2.3`
/// - `./downloads/my-package.tgz` → `my-package`
/// - `/tmp/foo.tar.gz` → `foo`
pub fn name_from_url(url: &str) -> Option<String> {
    let filename = url.rsplit('/').next().filter(|s| !s.is_empty())?;
    let stem = filename
        .strip_suffix(".tgz")
        .or_else(|| filename.strip_suffix(".tar.gz"))
        .or_else(|| filename.strip_suffix(".tar"))?;
    Some(stem.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    fn create_tarball_with_package_json(name: &str, version: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
        let mut ar = tar::Builder::new(encoder);

        let pkg_json = serde_json::json!({
            "name": name,
            "version": version,
        })
        .to_string();

        let mut header = tar::Header::new_gnu();
        header.set_path("package/package.json").unwrap();
        header.set_size(pkg_json.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        ar.append(&header, pkg_json.as_bytes()).unwrap();

        let encoder = ar.into_inner().unwrap();
        encoder.finish().unwrap();
        buf
    }

    fn create_tarball_without_package_json() -> Vec<u8> {
        let mut buf = Vec::new();
        let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
        let mut ar = tar::Builder::new(encoder);

        let mut header = tar::Header::new_gnu();
        header.set_path("package/index.js").unwrap();
        header.set_size(5u64);
        header.set_mode(0o644);
        header.set_cksum();
        ar.append(&header, &b"hello"[..]).unwrap();

        let encoder = ar.into_inner().unwrap();
        encoder.finish().unwrap();
        buf
    }

    #[test]
    fn test_identity_from_valid_tarball() {
        let tarball = create_tarball_with_package_json("my-pkg", "1.2.3");
        let (name, version) = identity_from_tarball(&tarball).unwrap();
        assert_eq!(name, "my-pkg");
        assert_eq!(version, "1.2.3");
    }

    #[test]
    fn test_identity_from_tarball_no_package_json() {
        let tarball = create_tarball_without_package_json();
        let err = identity_from_tarball(&tarball).unwrap_err();
        assert!(matches!(err, SourceError::PackageNotFound));
    }

    #[test]
    fn test_identity_from_tarball_invalid_json() {
        let mut buf = Vec::new();
        let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
        let mut ar = tar::Builder::new(encoder);

        let mut header = tar::Header::new_gnu();
        header.set_path("package/package.json").unwrap();
        header.set_size(4u64);
        header.set_mode(0o644);
        header.set_cksum();
        ar.append(&header, &b"null"[..]).unwrap();

        let encoder = ar.into_inner().unwrap();
        encoder.finish().unwrap();

        let err = identity_from_tarball(&buf).unwrap_err();
        assert!(matches!(err, SourceError::PackageNotFound));
    }

    #[test]
    fn test_identity_from_tarball_missing_name() {
        let mut buf = Vec::new();
        let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
        let mut ar = tar::Builder::new(encoder);

        let pkg_json = serde_json::json!({ "version": "1.0.0" }).to_string();
        let mut header = tar::Header::new_gnu();
        header.set_path("package/package.json").unwrap();
        header.set_size(pkg_json.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        ar.append(&header, pkg_json.as_bytes()).unwrap();

        let encoder = ar.into_inner().unwrap();
        encoder.finish().unwrap();

        let err = identity_from_tarball(&buf).unwrap_err();
        assert!(matches!(err, SourceError::PackageNotFound));
    }

    #[test]
    fn test_identity_from_tarball_missing_version_defaults() {
        let mut buf = Vec::new();
        let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
        let mut ar = tar::Builder::new(encoder);

        let pkg_json = serde_json::json!({ "name": "foo" }).to_string();
        let mut header = tar::Header::new_gnu();
        header.set_path("package/package.json").unwrap();
        header.set_size(pkg_json.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        ar.append(&header, pkg_json.as_bytes()).unwrap();

        let encoder = ar.into_inner().unwrap();
        encoder.finish().unwrap();

        let (name, version) = identity_from_tarball(&buf).unwrap();
        assert_eq!(name, "foo");
        assert_eq!(version, "0.0.0");
    }

    #[test]
    fn test_name_from_url_tgz() {
        let name = name_from_url("https://example.com/pkg-1.2.3.tgz").unwrap();
        assert_eq!(name, "pkg-1.2.3");
    }

    #[test]
    fn test_name_from_url_tar_gz() {
        let name = name_from_url("https://example.com/my-package.tar.gz").unwrap();
        assert_eq!(name, "my-package");
    }

    #[test]
    fn test_name_from_url_local_path() {
        let name = name_from_url("./downloads/foo.tgz").unwrap();
        assert_eq!(name, "foo");
    }

    #[test]
    fn test_name_from_url_no_extension() {
        let result = name_from_url("https://example.com/package");
        assert!(result.is_none());
    }
}
