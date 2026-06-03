use std::path::Path;

use flate2::write::GzEncoder;
use flate2::Compression;

use crate::source::SourceError;
use crate::types::PackageIdentity;

pub struct LocalSource {
    pub path: String,
}

impl LocalSource {
    #[must_use]
    pub const fn new(path: String) -> Self {
        Self { path }
    }

    #[allow(clippy::unnecessary_wraps)]
    pub fn resolve(&self, _name: &str) -> Result<String, SourceError> {
        Ok(self.path.clone())
    }

    pub fn fetch(&self, _identity: &PackageIdentity) -> Result<Vec<u8>, SourceError> {
        let dir = Path::new(&self.path);
        let mut buf = Vec::new();
        let encoder = GzEncoder::new(&mut buf, Compression::best());
        let mut ar = tar::Builder::new(encoder);
        ar.append_dir_all(".", dir)?;
        let encoder = ar.into_inner()?;
        encoder.finish()?;
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::types::{SourceType, Version};
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_temp_package() -> TempDir {
        let dir = TempDir::new().expect("failed to create temp dir");
        let mut f = File::create(dir.path().join("ara.toml")).unwrap();
        writeln!(f, "[project]\nname = \"test-pkg\"\nversion = \"0.1.0\"").unwrap();
        dir
    }

    #[test]
    fn test_fetch_produces_valid_tar() {
        let tmp = create_temp_package();
        let path = tmp.path().to_str().unwrap().to_string();
        let src = LocalSource::new(path);

        let id = PackageIdentity {
            source: SourceType::Local,
            name: "test-pkg".to_string(),
            version: Version::parse("0.1.0").unwrap(),
            content_hash: None,
        };

        let tarball = src.fetch(&id).unwrap();
        assert!(tarball.len() > 0);
        assert!(tarball.len() > 64);
        assert_eq!(tarball[0], 0x1f); // gzip magic
        assert_eq!(tarball[1], 0x8b);
    }
}
