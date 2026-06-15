use std::collections::BTreeMap;

use crate::types::{
    Build, DepEntryRaw, DependencyEntry, Manifest, Project, ScriptEntry, Security, Workspace,
};
use ara_types::Constraint;

#[derive(Debug, thiserror::Error)]
pub enum ManifestParseError {
    #[error(
        "unknown source type: '{0}'. valid types: npm, registry, github, git, local, workspace"
    )]
    UnknownSourceType(String),
    #[error("invalid risk level: '{0}'. valid levels: low, medium, high, critical")]
    InvalidRiskLevel(String),
    #[error("invalid name: {0}")]
    InvalidName(String),
    #[error("invalid version constraint: {0}")]
    InvalidConstraint(String),
    #[error("json parse error at line {line}, column {col}: {message}")]
    Json {
        line: usize,
        col: usize,
        message: String,
    },
    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),
}

#[derive(serde::Deserialize)]
struct ManifestRaw {
    project: Option<ProjectRaw>,
    deps: Option<BTreeMap<String, DepEntryRaw>>,
    workspace: Option<WorkspaceRaw>,
    scripts: Option<BTreeMap<String, String>>,
    security: Option<SecurityRaw>,
    build: Option<BuildRaw>,
}

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct ProjectRaw {
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
    license: Option<String>,
    repository: Option<String>,
    homepage: Option<String>,
}

#[derive(serde::Deserialize)]
struct WorkspaceRaw {
    members: Option<Vec<String>>,
    #[allow(dead_code)]
    catalog: Option<std::collections::HashMap<String, String>>,
    #[allow(dead_code)]
    catalogs: Option<std::collections::HashMap<String, std::collections::HashMap<String, String>>>,
}

#[derive(serde::Deserialize)]
struct SecurityRaw {
    risk_threshold: Option<String>,
    require_review: Option<bool>,
}

#[derive(serde::Deserialize)]
struct BuildRaw {
    hermetic: Option<bool>,
    offline_first: Option<bool>,
}

fn validate_name(name: &str) -> Result<(), ManifestParseError> {
    if name.is_empty() {
        return Err(ManifestParseError::InvalidName("name is empty".to_string()));
    }
    if name.contains('\0') {
        return Err(ManifestParseError::InvalidName(
            "name contains null byte".to_string(),
        ));
    }
    if name.starts_with('/') {
        return Err(ManifestParseError::InvalidName(
            "name must not be an absolute path".to_string(),
        ));
    }
    for component in name.split('/') {
        if component == ".." || component == "." {
            return Err(ManifestParseError::InvalidName(
                "name must not contain path traversal segments".to_string(),
            ));
        }
    }
    Ok(())
}

pub fn parse(content: &str) -> Result<Manifest, ManifestParseError> {
    let raw: ManifestRaw = toml::from_str(content)?;

    let project = match raw.project {
        Some(p) => {
            let name = p
                .name
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "project".to_string());
            validate_name(&name)?;
            let version = p
                .version
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "0.0.0".to_string());
            Project { name, version }
        }
        None => Project {
            name: "project".to_string(),
            version: "0.0.0".to_string(),
        },
    };

    let valid_sources = ["npm", "registry", "github", "git", "local", "workspace"];

    let deps = match raw.deps {
        Some(map) => map
            .into_iter()
            .map(|(name, raw)| {
                validate_name(&name)?;
                let source = raw.source.unwrap_or_else(|| "npm".to_string());
                if !valid_sources.contains(&source.as_str()) {
                    return Err(ManifestParseError::UnknownSourceType(source));
                }
                if let Some(ref ver) = raw.version {
                    if !ver.is_empty() {
                        Constraint::parse(ver).map_err(|e| {
                            ManifestParseError::InvalidConstraint(format!("{}: {}", name, e))
                        })?;
                    }
                }
                Ok(DependencyEntry {
                    name,
                    source,
                    kind: raw.kind,
                    version: raw.version,
                    repo: raw.repo,
                    url: raw.url,
                    commit: raw.commit,
                    path: raw.path,
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };

    let valid_risk_levels = ["low", "medium", "high", "critical"];

    let security = raw
        .security
        .map(|s| {
            if let Some(ref threshold) = s.risk_threshold {
                if !valid_risk_levels.contains(&threshold.as_str()) {
                    return Err(ManifestParseError::InvalidRiskLevel(threshold.clone()));
                }
            }
            Ok(Security {
                risk_threshold: s.risk_threshold,
                require_review: s.require_review,
            })
        })
        .transpose()?;

    let workspace = raw.workspace.map(|w| Workspace {
        members: w.members.unwrap_or_default(),
        catalog: w.catalog,
        catalogs: w.catalogs,
    });

    let scripts = raw.scripts.map_or_else(Vec::new, |map| {
        map.into_iter()
            .map(|(name, command)| ScriptEntry { name, command })
            .collect()
    });

    let build = raw.build.map(|b| Build {
        hermetic: b.hermetic,
        offline_first: b.offline_first,
    });

    Ok(Manifest {
        project,
        deps,
        workspace,
        scripts,
        security,
        build,
        package_json_extras: None,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_parse_minimal() {
        let src = r#"
            [project]
            name = "my-app"
            version = "0.1.0"
        "#;
        let m = parse(src).unwrap();
        assert_eq!(m.project.name, "my-app");
        assert_eq!(m.project.version, "0.1.0");
    }

    #[test]
    fn test_parse_with_deps() {
        let src = r#"
            [project]
            name = "app"
            version = "1.0.0"

            [deps]
            zod = { source = "npm", version = "3.23.8" }
            react = { source = "github", repo = "facebook/react", version = "18.x" }
        "#;
        let m = parse(src).unwrap();
        assert_eq!(m.deps.len(), 2);
        let zod = m.deps.iter().find(|d| d.name == "zod").unwrap();
        assert_eq!(zod.source, "npm");
        assert_eq!(zod.version.as_deref(), Some("3.23.8"));
        let react = m.deps.iter().find(|d| d.name == "react").unwrap();
        assert_eq!(react.repo.as_deref(), Some("facebook/react"));
    }

    #[test]
    fn test_parse_workspace() {
        let src = r#"
            [project]
            name = "monorepo"
            version = "0.1.0"

            [workspace]
            members = ["apps/*", "packages/*"]
        "#;
        let m = parse(src).unwrap();
        assert!(m.workspace.is_some());
        assert_eq!(m.workspace.as_ref().unwrap().members.len(), 2);
        assert_eq!(m.workspace.as_ref().unwrap().members[0], "apps/*");
    }

    #[test]
    fn test_parse_unknown_source_type() {
        let src = r#"
            [project]
            name = "app"
            version = "0.1.0"

            [deps]
            foo = { source = "nonexistent", version = "1.0.0" }
        "#;
        match parse(src) {
            Err(ManifestParseError::UnknownSourceType(_)) => {}
            other => panic!("expected UnknownSourceType, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_invalid_risk_level() {
        let src = r#"
            [project]
            name = "app"
            version = "0.1.0"

            [security]
            risk_threshold = "bogus"
        "#;
        match parse(src) {
            Err(ManifestParseError::InvalidRiskLevel(_)) => {}
            other => panic!("expected InvalidRiskLevel, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_security_and_build() {
        let src = r#"
            [project]
            name = "secure-app"
            version = "0.1.0"

            [security]
            risk_threshold = "medium"
            require_review = true

            [build]
            hermetic = true
            offline_first = true
        "#;
        let m = parse(src).unwrap();
        assert_eq!(
            m.security.as_ref().unwrap().risk_threshold.as_deref(),
            Some("medium")
        );
        assert_eq!(m.security.as_ref().unwrap().require_review, Some(true));
        assert_eq!(m.build.as_ref().unwrap().hermetic, Some(true));
        assert_eq!(m.build.as_ref().unwrap().offline_first, Some(true));
    }

    #[test]
    fn test_parse_scripts() {
        let src = r#"
            [project]
            name = "app"
            version = "0.1.0"

            [scripts]
            build = "tsc"
            test = "vitest"
        "#;
        let m = parse(src).unwrap();
        assert_eq!(m.scripts.len(), 2);
        assert_eq!(m.scripts[0].name, "build");
        assert_eq!(m.scripts[0].command, "tsc");
        assert_eq!(m.scripts[1].name, "test");
        assert_eq!(m.scripts[1].command, "vitest");
    }

    #[test]
    fn test_parse_with_kind_field() {
        let src = r#"
            [project]
            name = "app"
            version = "1.0.0"

            [deps]
            prod-dep = { version = "1.0.0" }
            dev-dep = { version = "2.0.0", kind = "dev" }
            peer-dep = { version = "3.0.0", kind = "peer" }
            opt-dep = { version = "4.0.0", kind = "optional" }
        "#;
        let m = parse(src).unwrap();
        assert_eq!(m.deps.len(), 4);

        let prod = m.deps.iter().find(|d| d.name == "prod-dep").unwrap();
        assert_eq!(prod.kind.as_deref(), None);

        let dev = m.deps.iter().find(|d| d.name == "dev-dep").unwrap();
        assert_eq!(dev.kind.as_deref(), Some("dev"));

        let peer = m.deps.iter().find(|d| d.name == "peer-dep").unwrap();
        assert_eq!(peer.kind.as_deref(), Some("peer"));

        let opt = m.deps.iter().find(|d| d.name == "opt-dep").unwrap();
        assert_eq!(opt.kind.as_deref(), Some("optional"));
    }

    #[test]
    fn test_parse_unicode_name() {
        let src = r#"
            [project]
            name = "🔥-test-中文"
            version = "0.1.0"
        "#;
        let m = parse(src).unwrap();
        assert_eq!(m.project.name, "🔥-test-中文");
    }

    #[test]
    fn test_parse_long_name() {
        let name = "a".repeat(10_000);
        let src = format!(
            r#"
            [project]
            name = "{name}"
            version = "0.1.0"
        "#
        );
        let m = parse(&src).unwrap();
        assert_eq!(m.project.name.len(), 10_000);
    }

    #[test]
    fn test_parse_deeply_nested_table() {
        let src = r#"
            [project]
            name = "weird"
            version = "0.1.0"

            [deps]
            pkg = { source = "npm", version = "1.0.0" }

            [a.b.c.d.e.f.g]
            x = 1
        "#;
        parse(src).unwrap();
    }

    #[test]
    fn test_parse_name_with_path_chars() {
        let src = r#"
            [project]
            name = "../../etc/passwd"
            version = "0.1.0"
        "#;
        match parse(src) {
            Err(ManifestParseError::InvalidName(_)) => {}
            other => panic!("expected InvalidName, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_invalid_constraint() {
        let src = r#"
            [project]
            name = "app"
            version = "0.1.0"

            [deps]
            foo = { version = "invalid!!" }
        "#;
        match parse(src) {
            Err(ManifestParseError::InvalidConstraint(_)) => {}
            other => panic!("expected InvalidConstraint, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_huge_integer() {
        let src = r#"
            [project]
            name = "bigint"
            version = "0.1.0"

            [security]
            require_review = true
        "#;
        // TOML integers beyond i64 range are ignored when deserializing
        // into a struct without a matching field (toml 1.0+ behavior).
        let huge = format!("{src}\nhuge = 999999999999999999999999999");
        let result = parse(&huge);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_workspace_catalog_default() {
        let src = r#"
            [project]
            name = "monorepo"
            version = "0.1.0"

            [workspace]
            members = ["packages/*"]

            [workspace.catalog]
            react = "^19.0.0"
            react-dom = "^19.0.0"
        "#;
        let m = parse(src).unwrap();
        let ws = m.workspace.as_ref().unwrap();
        let cat = ws.catalog.as_ref().unwrap();
        assert_eq!(cat.get("react").unwrap(), "^19.0.0");
        assert_eq!(cat.get("react-dom").unwrap(), "^19.0.0");
        assert!(cat.get("nonexistent").is_none());
    }

    #[test]
    fn test_parse_workspace_catalog_with_named() {
        let src = r#"
            [project]
            name = "monorepo"
            version = "0.1.0"

            [workspace]
            members = ["packages/*"]

            [workspace.catalog]
            react = "^19.0.0"

            [workspace.catalogs.testing]
            jest = "30.0.0"
            vitest = "^1.0.0"
        "#;
        let m = parse(src).unwrap();
        let ws = m.workspace.as_ref().unwrap();

        let cat = ws.catalog.as_ref().unwrap();
        assert_eq!(cat.get("react").unwrap(), "^19.0.0");

        let testing = &ws.catalogs.as_ref().unwrap()["testing"];
        assert_eq!(testing.get("jest").unwrap(), "30.0.0");
        assert_eq!(testing.get("vitest").unwrap(), "^1.0.0");
    }

    #[test]
    fn test_parse_workspace_catalog_only_named() {
        let src = r#"
            [project]
            name = "monorepo"
            version = "0.1.0"

            [workspace]
            members = ["packages/*"]
            catalog = { zod = "^3.23.0" }

            [workspace.catalogs.build]
            typescript = "^5.0.0"
        "#;
        let m = parse(src).unwrap();
        let ws = m.workspace.as_ref().unwrap();

        let cat = ws.catalog.as_ref().unwrap();
        assert_eq!(cat.get("zod").unwrap(), "^3.23.0");

        let build = &ws.catalogs.as_ref().unwrap()["build"];
        assert_eq!(build.get("typescript").unwrap(), "^5.0.0");
    }

    #[test]
    fn test_parse_workspace_no_catalog() {
        let src = r#"
            [project]
            name = "simple"
            version = "0.1.0"

            [workspace]
            members = ["packages/*"]
        "#;
        let m = parse(src).unwrap();
        let ws = m.workspace.as_ref().unwrap();
        assert!(ws.catalog.is_none());
        assert!(ws.catalogs.is_none());
    }
}
