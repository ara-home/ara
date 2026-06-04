use std::collections::BTreeMap;

use crate::manifest::types::{
    Build, DepEntryRaw, DependencyEntry, Manifest, Project, ScriptEntry, Security, Workspace,
};

#[derive(Debug, thiserror::Error)]
pub enum ManifestParseError {
    #[error("missing project name")]
    MissingProjectName,
    #[error("missing project version")]
    MissingProjectVersion,
    #[error("unknown source type")]
    UnknownSourceType,
    #[error("invalid risk level")]
    InvalidRiskLevel,
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

pub fn parse(content: &str) -> Result<Manifest, ManifestParseError> {
    let raw: ManifestRaw = toml::from_str(content)?;

    let project = match raw.project {
        Some(p) => {
            let name = p.name.unwrap_or_default();
            let version = p.version.unwrap_or_default();
            if name.is_empty() {
                return Err(ManifestParseError::MissingProjectName);
            }
            if version.is_empty() {
                return Err(ManifestParseError::MissingProjectVersion);
            }
            Project {
                name,
                version,
            }
        }
        None => return Err(ManifestParseError::MissingProjectName),
    };

    let valid_sources = ["npm", "registry", "github", "git", "local", "workspace"];

    let deps = match raw.deps {
        Some(map) => map
            .into_iter()
            .map(|(name, raw)| {
                let source = raw.source.unwrap_or_else(|| "npm".to_string());
                if !valid_sources.contains(&source.as_str()) {
                    return Err(ManifestParseError::UnknownSourceType);
                }
                Ok(DependencyEntry {
                    name,
                    source,
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
                    return Err(ManifestParseError::InvalidRiskLevel);
                }
            }
            Ok(Security {
                risk_threshold: s.risk_threshold,
                require_review: s.require_review,
            })
        })
        .transpose()?;

    let workspace = raw
        .workspace
        .and_then(|w| w.members.map(|members| Workspace { members }));

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
    fn test_parse_missing_project_section() {
        let src = r#"
            name = "no-project"
            version = "0.1.0"
        "#;
        match parse(src) {
            Err(ManifestParseError::MissingProjectName) => {}
            other => panic!("expected MissingProjectName, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_missing_project_version() {
        let src = r#"
            [project]
            name = "app"
        "#;
        match parse(src) {
            Err(ManifestParseError::MissingProjectVersion) => {}
            other => panic!("expected MissingProjectVersion, got {other:?}"),
        }
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
            Err(ManifestParseError::UnknownSourceType) => {}
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
            Err(ManifestParseError::InvalidRiskLevel) => {}
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
        let m = parse(src).unwrap();
        assert_eq!(m.project.name, "../../etc/passwd");
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
        // TOML integers beyond i64 range cause parse error
        let huge = format!("{src}\nhuge = 999999999999999999999999999");
        let result = parse(&huge);
        assert!(result.is_err());
    }
}
