use crate::lockfile::types::Lockfile;

#[derive(Debug, thiserror::Error)]
pub enum LockfileParseError {
    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),
}

pub fn parse(content: &str) -> Result<Lockfile, LockfileParseError> {
    let lockfile: Lockfile = toml::from_str(content)?;
    Ok(lockfile)
}

#[cfg(test)]
mod tests {
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
}
