use crate::lockfile::types::Lockfile;

#[derive(Debug, thiserror::Error)]
pub enum LockfileParseError {
    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("unsupported lockfile version: {0}")]
    UnsupportedVersion(u32),
    #[error("unknown resolver: {0}")]
    UnknownResolver(String),
    #[error("invalid package entry: {0}")]
    InvalidPackage(String),
}

pub fn parse(content: &str) -> Result<Lockfile, LockfileParseError> {
    let lockfile: Lockfile = toml::from_str(content)?;

    if lockfile.version != 1 {
        return Err(LockfileParseError::UnsupportedVersion(lockfile.version));
    }
    if lockfile.graph.resolver != "mvs" {
        return Err(LockfileParseError::UnknownResolver(
            lockfile.graph.resolver.clone(),
        ));
    }
    for (i, pkg) in lockfile.packages.iter().enumerate() {
        if pkg.name.is_empty() {
            return Err(LockfileParseError::InvalidPackage(format!(
                "package {}: name is empty",
                i
            )));
        }
        if pkg.version.is_empty() {
            return Err(LockfileParseError::InvalidPackage(format!(
                "package {} ({}): version is empty",
                i, pkg.name
            )));
        }
        if pkg.source.is_empty() {
            return Err(LockfileParseError::InvalidPackage(format!(
                "package {} ({}): source is empty",
                i, pkg.name
            )));
        }
        if pkg.package_hash.is_empty() {
            return Err(LockfileParseError::InvalidPackage(format!(
                "package {} ({}): package_hash is empty",
                i, pkg.name
            )));
        }
    }

    Ok(lockfile)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_parse_minimal() {
        let src = r#"
            version = 1

            [graph]
            resolver = "mvs"
            graph_hash = "sha256:abc"

            [[package]]
            name = "zod"
            version = "3.23.8"
            source = "npm"
            package_hash = "sha256:def"
        "#;
        let lf = parse(src).unwrap();
        assert_eq!(lf.version, 1);
        assert_eq!(lf.graph.resolver, "mvs");
        assert_eq!(lf.graph.graph_hash.as_deref(), Some("sha256:abc"));
        assert_eq!(lf.packages.len(), 1);
        assert_eq!(lf.packages[0].name, "zod");
        assert_eq!(lf.packages[0].source, "npm");
    }

    #[test]
    fn test_parse_with_graph_meta() {
        let src = r#"
            version = 1

            [graph]
            resolver = "mvs"
            generated_at = "2026-06-01T22:00:00Z"
            graph_hash = "sha256:789"

            [[package]]
            name = "react"
            version = "18.3.0"
            source = "github"
            package_hash = "sha256:xyz"
        "#;
        let lf = parse(src).unwrap();
        assert_eq!(lf.graph.resolver, "mvs");
        assert_eq!(
            lf.graph.generated_at.as_deref(),
            Some("2026-06-01T22:00:00Z")
        );
        assert_eq!(lf.graph.graph_hash.as_deref(), Some("sha256:789"));
        assert_eq!(lf.packages.len(), 1);
        assert_eq!(lf.packages[0].name, "react");
    }

    #[test]
    fn test_parse_empty_packages() {
        let src = r#"
            version = 1

            [graph]
            resolver = "mvs"
        "#;
        let lf = parse(src).unwrap();
        assert!(lf.packages.is_empty());
    }

    #[test]
    fn test_parse_unsupported_version() {
        let src = r#"
            version = 2

            [graph]
            resolver = "mvs"
        "#;
        match parse(src) {
            Err(LockfileParseError::UnsupportedVersion(2)) => {}
            other => panic!("expected UnsupportedVersion(2), got {other:?}"),
        }
    }

    #[test]
    fn test_parse_unknown_resolver() {
        let src = r#"
            version = 1

            [graph]
            resolver = "cargo"
        "#;
        match parse(src) {
            Err(LockfileParseError::UnknownResolver(ref r)) if r == "cargo" => {}
            other => panic!("expected UnknownResolver, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_empty_package_name() {
        let src = r#"
            version = 1

            [graph]
            resolver = "mvs"

            [[package]]
            name = ""
            version = "1.0.0"
            source = "npm"
            package_hash = "sha256:abc"
        "#;
        match parse(src) {
            Err(LockfileParseError::InvalidPackage(_)) => {}
            other => panic!("expected InvalidPackage, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_empty_package_hash() {
        let src = r#"
            version = 1

            [graph]
            resolver = "mvs"

            [[package]]
            name = "foo"
            version = "1.0.0"
            source = "npm"
            package_hash = ""
        "#;
        match parse(src) {
            Err(LockfileParseError::InvalidPackage(_)) => {}
            other => panic!("expected InvalidPackage, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_empty_package_version() {
        let src = r#"
            version = 1

            [graph]
            resolver = "mvs"

            [[package]]
            name = "foo"
            version = ""
            source = "npm"
            package_hash = "sha256:abc"
        "#;
        match parse(src) {
            Err(LockfileParseError::InvalidPackage(ref msg)) if msg.contains("version is empty") => {}
            other => panic!("expected InvalidPackage version is empty, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_empty_package_source() {
        let src = r#"
            version = 1

            [graph]
            resolver = "mvs"

            [[package]]
            name = "foo"
            version = "1.0.0"
            source = ""
            package_hash = "sha256:abc"
        "#;
        match parse(src) {
            Err(LockfileParseError::InvalidPackage(ref msg)) if msg.contains("source is empty") => {}
            other => panic!("expected InvalidPackage source is empty, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_invalid_toml() {
        let src = r#"
            version = 1
            [graph]
            resolver = "mvs
        "#;
        match parse(src) {
            Err(LockfileParseError::Toml(_)) => {}
            other => panic!("expected Toml error, got {other:?}"),
        }
    }
}
