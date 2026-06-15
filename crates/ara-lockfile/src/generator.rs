use std::fmt::Write;

use crate::types::Lockfile;

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
    if let Some(ws) = &lockfile.workspace {
        if let Some(cat) = &ws.catalog {
            out.push_str("[workspace.catalog]\n");
            for (k, v) in cat {
                let _ = writeln!(&mut out, "{k} = \"{v}\"");
            }
            out.push('\n');
        }
        if let Some(cats) = &ws.catalogs {
            for (name, entries) in cats {
                let _ = writeln!(&mut out, "[workspace.catalogs.{name}]");
                for (k, v) in entries {
                    let _ = writeln!(&mut out, "{k} = \"{v}\"");
                }
            }
            out.push('\n');
        }
    }

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
        if let Some(sec) = &pkg.security {
            out.push_str("[package.security]\n");
            if let Some(rl) = &sec.risk_level {
                let _ = writeln!(&mut out, "risk_level = \"{rl}\"");
            }
        }
        if let Some(sbom) = &pkg.sbom {
            out.push_str("[package.sbom]\n");
            if let Some(lic) = &sbom.license {
                let _ = writeln!(&mut out, "license = \"{lic}\"");
            }
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::types::{GraphMeta, LockfileWorkspace, PackageEntry, SbomMeta, SecurityMeta};

    #[test]
    fn test_generate_and_parse_back() {
        let lf = Lockfile {
            version: 1,
            graph: GraphMeta {
                resolver: "mvs".to_string(),
                generated_at: Some("2026-06-01T22:00:00Z".to_string()),
                graph_hash: Some("sha256:abc".to_string()),
            },
            workspace: None,
            packages: vec![PackageEntry {
                name: "zod".to_string(),
                version: "3.23.8".to_string(),
                source: "npm".to_string(),
                package_hash:
                    "sha256-2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                        .to_string(),
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

        let parsed = crate::parser::parse(&output).unwrap();
        assert_eq!(parsed.packages[0].name, "zod");
        assert_eq!(parsed.graph.resolver, "mvs");
    }

    #[test]
    fn test_generate_with_catalog() {
        let cat = std::collections::HashMap::from([("react".to_string(), "^19.0.0".to_string())]);
        let testing = std::collections::HashMap::from([("jest".to_string(), "30.0.0".to_string())]);
        let cats = std::collections::HashMap::from([("testing".to_string(), testing)]);

        let lf = Lockfile {
            version: 1,
            graph: GraphMeta {
                resolver: "mvs".to_string(),
                generated_at: None,
                graph_hash: None,
            },
            workspace: Some(LockfileWorkspace {
                catalog: Some(cat),
                catalogs: Some(cats),
            }),
            packages: vec![],
        };

        let output = generate(&lf);
        assert!(output.contains("[workspace.catalog]"));
        assert!(output.contains(r#"react = "^19.0.0""#));
        assert!(output.contains("[workspace.catalogs.testing]"));
        assert!(output.contains(r#"jest = "30.0.0""#));

        // Round-trip: parse back
        let parsed = crate::parser::parse(&output).unwrap();
        let ws = parsed.workspace.as_ref().unwrap();
        assert_eq!(
            ws.catalog.as_ref().unwrap().get("react").unwrap(),
            "^19.0.0"
        );
        assert_eq!(
            ws.catalogs
                .as_ref()
                .unwrap()
                .get("testing")
                .unwrap()
                .get("jest")
                .unwrap(),
            "30.0.0"
        );
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
            workspace: None,
            packages: vec![PackageEntry {
                name: "react".to_string(),
                version: "18.3.0".to_string(),
                source: "github".to_string(),
                package_hash:
                    "sha256-2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                        .to_string(),
                integrity: Some("sha256-xyz".to_string()),
                signature: None,
                repository: Some("facebook/react".to_string()),
                commit: Some("abc123".to_string()),
                dependencies: Some(vec!["shared".to_string()]),
                security: Some(SecurityMeta {
                    risk_level: Some("medium".to_string()),
                }),
                sbom: Some(SbomMeta {
                    license: Some("MIT".to_string()),
                }),
            }],
        };

        let output = generate(&lf);
        assert!(output.contains("facebook/react"));
        assert!(output.contains("abc123"));
        assert!(output.contains("shared"));
        assert!(output.contains("risk_level"));
        assert!(output.contains("medium"));
        assert!(output.contains("license"));
        assert!(output.contains("MIT"));

        // Verify round-trip: generate -> parse preserves new fields
        let parsed = crate::parser::parse(&output).unwrap();
        let pkg = &parsed.packages[0];
        assert_eq!(
            pkg.security.as_ref().unwrap().risk_level.as_deref(),
            Some("medium")
        );
        assert_eq!(pkg.sbom.as_ref().unwrap().license.as_deref(), Some("MIT"));
    }
}
