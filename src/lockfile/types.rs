#![allow(dead_code)]

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct GraphMeta {
    #[serde(default = "default_resolver")]
    pub resolver: String,
    pub generated_at: Option<String>,
    pub graph_hash: Option<String>,
}

fn default_resolver() -> String {
    "mvs".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityMeta {
    pub risk_level: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SbomMeta {
    pub license: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageEntry {
    pub name: String,
    pub version: String,
    pub source: String,
    pub package_hash: String,
    pub integrity: Option<String>,
    pub signature: Option<String>,
    pub repository: Option<String>,
    pub commit: Option<String>,
    pub dependencies: Option<Vec<String>>,
    pub security: Option<SecurityMeta>,
    pub sbom: Option<SbomMeta>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Lockfile {
    #[serde(default)]
    pub version: u32,
    #[serde(default = "default_graph_meta")]
    pub graph: GraphMeta,
    #[serde(default)]
    #[serde(rename = "package")]
    pub packages: Vec<PackageEntry>,
}

fn default_graph_meta() -> GraphMeta {
    GraphMeta {
        resolver: "mvs".to_string(),
        generated_at: None,
        graph_hash: None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_deserialize_minimal() {
        let toml_str = r#"
            version = 1
            [graph]
            resolver = "mvs"
        "#;
        let lf: Lockfile = toml::from_str(toml_str).unwrap();
        assert_eq!(lf.version, 1);
        assert_eq!(lf.graph.resolver, "mvs");
        assert!(lf.graph.generated_at.is_none());
        assert!(lf.packages.is_empty());
    }

    #[test]
    fn test_deserialize_with_packages() {
        let toml_str = r#"
            version = 1

            [graph]
            resolver = "mvs"
            generated_at = "2025-01-01T00:00:00Z"
            graph_hash = "sha256:abc"

            [[package]]
            name = "zod"
            version = "3.23.8"
            source = "npm"
            package_hash = "sha256-def"
        "#;
        let lf: Lockfile = toml::from_str(toml_str).unwrap();
        assert_eq!(lf.packages.len(), 1);
        assert_eq!(lf.packages[0].name, "zod");
        assert_eq!(lf.packages[0].version, "3.23.8");
        assert_eq!(
            lf.graph.generated_at.as_deref(),
            Some("2025-01-01T00:00:00Z")
        );
    }

    #[test]
    fn test_deserialize_default_graph() {
        let toml_str = r#"
            version = 1
        "#;
        let lf: Lockfile = toml::from_str(toml_str).unwrap();
        assert_eq!(lf.graph.resolver, "mvs");
        assert!(lf.graph.generated_at.is_none());
        assert!(lf.graph.graph_hash.is_none());
    }

    #[test]
    fn test_deserialize_package_with_all_fields() {
        let toml_str = r#"
            version = 1
            [graph]
            resolver = "mvs"

            [[package]]
            name = "lib"
            version = "1.0.0"
            source = "github"
            package_hash = "sha256-xxx"
            integrity = "sha256-yyy"
            signature = "sig123"
            repository = "https://github.com/user/repo"
            commit = "abc123"
            dependencies = ["dep1", "dep2"]

            [package.security]
            risk_level = "low"
            analysis_version = "1.0.0"

            [package.sbom]
            license = "MIT"
            supplier = "test"
        "#;
        let lf: Lockfile = toml::from_str(toml_str).unwrap();
        let pkg = &lf.packages[0];
        assert_eq!(pkg.name, "lib");
        assert_eq!(pkg.integrity.as_deref(), Some("sha256-yyy"));
        assert_eq!(pkg.dependencies.as_ref().unwrap().len(), 2);
        assert_eq!(
            pkg.security.as_ref().unwrap().risk_level.as_deref(),
            Some("low")
        );
        assert_eq!(pkg.sbom.as_ref().unwrap().license.as_deref(), Some("MIT"));
    }
}
