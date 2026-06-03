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
    allow_lifecycle_scripts: Option<bool>,
    block_critical: Option<bool>,
}

#[derive(serde::Deserialize)]
struct BuildRaw {
    hermetic: Option<bool>,
    offline_first: Option<bool>,
}

pub fn parse(content: &str) -> Result<Manifest, ManifestParseError> {
    let raw: ManifestRaw = toml::from_str(content)?;

    let project = match raw.project {
        Some(p) => Project {
            name: p.name.unwrap_or_default(),
            version: p.version.unwrap_or_default(),
            description: p.description,
            license: p.license,
            repository: p.repository,
            homepage: p.homepage,
        },
        None => Project {
            name: String::new(),
            version: String::new(),
            description: None,
            license: None,
            repository: None,
            homepage: None,
        },
    };

    let deps = match raw.deps {
        Some(map) => map
            .into_iter()
            .map(|(name, raw)| {
                let source = raw.source.unwrap_or_else(|| "npm".to_string());
                DependencyEntry {
                    name,
                    source,
                    version: raw.version,
                    repo: raw.repo,
                    url: raw.url,
                    commit: raw.commit,
                    path: raw.path,
                    package: raw.package,
                }
            })
            .collect(),
        None => Vec::new(),
    };

    let workspace = raw.workspace.and_then(|w| {
        w.members.map(|members| Workspace { members })
    });

    let scripts = match raw.scripts {
        Some(map) => map
            .into_iter()
            .map(|(name, command)| ScriptEntry { name, command })
            .collect(),
        None => Vec::new(),
    };

    let security = raw.security.map(|s| Security {
        risk_threshold: s.risk_threshold,
        require_review: s.require_review,
        allow_lifecycle_scripts: s.allow_lifecycle_scripts,
        block_critical: s.block_critical,
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
        let m = parse(src).unwrap();
        assert!(m.project.name.is_empty());
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
}
