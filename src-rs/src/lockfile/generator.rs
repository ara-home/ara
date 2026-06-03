use std::fmt::Write;

use crate::lockfile::types::Lockfile;

pub fn generate(lockfile: &Lockfile) -> String {
    let mut out = String::new();

    let _ = write!(&mut out, "version = {}\n\n", lockfile.version);

    out.push_str("[graph]\n");
    let _ = writeln!(&mut out, "resolver = \"{}\"", lockfile.graph.resolver);
    if let Some(t) = &lockfile.graph.generated_at {
        let _ = writeln!(&mut out, "generated_at = \"{t}\"");
    }
    if let Some(h) = &lockfile.graph.graph_hash {
        let _ = writeln!(&mut out, "graph_hash = \"{h}\"");
    }
    out.push('\n');

    for pkg in &lockfile.packages {
        out.push_str("[[package]]\n");
        let _ = writeln!(&mut out, "name = \"{}\"", pkg.name);
        let _ = writeln!(&mut out, "version = \"{}\"", pkg.version);
        let _ = writeln!(&mut out, "source = \"{}\"", pkg.source);
        if let Some(v) = &pkg.integrity {
            let _ = writeln!(&mut out, "integrity = \"{v}\"");
        }
        let _ = writeln!(&mut out, "package_hash = \"{}\"", pkg.package_hash);
        if let Some(v) = &pkg.signature {
            let _ = writeln!(&mut out, "signature = \"{v}\"");
        }
        if let Some(v) = &pkg.repository {
            let _ = writeln!(&mut out, "repository = \"{v}\"");
        }
        if let Some(v) = &pkg.commit {
            let _ = writeln!(&mut out, "commit = \"{v}\"");
        }
        if let Some(deps) = &pkg.dependencies {
            out.push_str("dependencies = [");
            for (i, dep) in deps.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let _ = write!(&mut out, "\"{dep}\"");
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
