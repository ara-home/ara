use crate::lockfile::types::Lockfile;

pub fn generate(lockfile: &Lockfile) -> String {
    let mut out = String::new();

    out.push_str(&format!("version = {}\n\n", lockfile.version));

    out.push_str("[graph]\n");
    out.push_str(&format!("resolver = \"{}\"\n", lockfile.graph.resolver));
    if let Some(t) = &lockfile.graph.generated_at {
        out.push_str(&format!("generated_at = \"{t}\"\n"));
    }
    if let Some(h) = &lockfile.graph.graph_hash {
        out.push_str(&format!("graph_hash = \"{h}\"\n"));
    }
    out.push('\n');

    for pkg in &lockfile.packages {
        out.push_str("[[package]]\n");
        out.push_str(&format!("name = \"{}\"\n", pkg.name));
        out.push_str(&format!("version = \"{}\"\n", pkg.version));
        out.push_str(&format!("source = \"{}\"\n", pkg.source));
        if let Some(v) = &pkg.integrity {
            out.push_str(&format!("integrity = \"{v}\"\n"));
        }
        out.push_str(&format!("package_hash = \"{}\"\n", pkg.package_hash));
        if let Some(v) = &pkg.signature {
            out.push_str(&format!("signature = \"{v}\"\n"));
        }
        if let Some(v) = &pkg.repository {
            out.push_str(&format!("repository = \"{v}\"\n"));
        }
        if let Some(v) = &pkg.commit {
            out.push_str(&format!("commit = \"{v}\"\n"));
        }
        if let Some(deps) = &pkg.dependencies {
            out.push_str("dependencies = [");
            for (i, dep) in deps.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("\"{dep}\""));
            }
            out.push_str("]\n");
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::lockfile::types::{GraphMeta, PackageEntry};

    #[test]
    fn test_generate_and_parse_back() {
        let lf = Lockfile {
            version: 1,
            graph: GraphMeta {
                resolver: "mvs".to_string(),
                generated_at: Some("2026-06-01T22:00:00Z".to_string()),
                graph_hash: Some("sha256:abc".to_string()),
            },
            packages: vec![PackageEntry {
                name: "zod".to_string(),
                version: "3.23.8".to_string(),
                source: "npm".to_string(),
                package_hash: "sha256:def".to_string(),
                integrity: None,
                signature: None,
                repository: None,
                commit: None,
                dependencies: None,
                security: None,
                sbom: None,
            }],
        };

        let output = generate(&lf);
        assert!(output.contains("zod"));
        assert!(output.contains("3.23.8"));
        assert!(output.contains("mvs"));

        let parsed = crate::lockfile::parser::parse(&output).unwrap();
        assert_eq!(parsed.packages[0].name, "zod");
        assert_eq!(parsed.graph.resolver, "mvs");
    }

    #[test]
    fn test_generate_with_all_fields() {
        let lf = Lockfile {
            version: 1,
            graph: GraphMeta {
                resolver: "mvs".to_string(),
                generated_at: Some("2026-06-01T22:00:00Z".to_string()),
                graph_hash: None,
            },
            packages: vec![PackageEntry {
                name: "react".to_string(),
                version: "18.3.0".to_string(),
                source: "github".to_string(),
                package_hash: "sha256:xyz".to_string(),
                integrity: Some("sha256:xyz".to_string()),
                signature: None,
                repository: Some("facebook/react".to_string()),
                commit: Some("abc123".to_string()),
                dependencies: Some(vec!["shared".to_string()]),
                security: None,
                sbom: None,
            }],
        };

        let output = generate(&lf);
        assert!(output.contains("facebook/react"));
        assert!(output.contains("abc123"));
        assert!(output.contains("shared"));
    }
}
