use std::collections::BTreeMap;

use serde::Deserialize;

use crate::manifest::parser::ManifestParseError;
use crate::manifest::types::{DependencyEntry, Manifest, Project, ScriptEntry, Workspace};

#[derive(Deserialize)]
#[allow(dead_code, non_snake_case)]
struct PackageJsonRaw {
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
    license: Option<String>,
    repository: Option<serde_json::Value>,
    homepage: Option<String>,
    bugs: Option<serde_json::Value>,
    author: Option<serde_json::Value>,
    keywords: Option<Vec<String>>,
    private: Option<bool>,
    dependencies: Option<BTreeMap<String, String>>,
    devDependencies: Option<BTreeMap<String, String>>,
    peerDependencies: Option<BTreeMap<String, String>>,
    optionalDependencies: Option<BTreeMap<String, String>>,
    scripts: Option<BTreeMap<String, String>>,
    workspaces: Option<Vec<String>>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[allow(dead_code)]
pub fn parse_package_json(content: &str) -> Result<Manifest, ManifestParseError> {
    let raw: PackageJsonRaw =
        serde_json::from_str(content).map_err(|e| ManifestParseError::Json(e.to_string()))?;

    let name = raw.name.unwrap_or_default();
    let version = raw.version.unwrap_or_default();
    if name.is_empty() {
        return Err(ManifestParseError::MissingProjectName);
    }
    if version.is_empty() {
        return Err(ManifestParseError::MissingProjectVersion);
    }

    let project = Project { name, version };

    let mut deps = Vec::new();

    if let Some(deps_map) = raw.dependencies {
        for (name, ver) in deps_map {
            deps.push(DependencyEntry {
                name,
                source: "npm".to_string(),
                kind: Some("prod".to_string()),
                version: Some(ver),
                repo: None,
                url: None,
                commit: None,
                path: None,
            });
        }
    }

    if let Some(deps_map) = raw.devDependencies {
        for (name, ver) in deps_map {
            deps.push(DependencyEntry {
                name,
                source: "npm".to_string(),
                kind: Some("dev".to_string()),
                version: Some(ver),
                repo: None,
                url: None,
                commit: None,
                path: None,
            });
        }
    }

    if let Some(deps_map) = raw.peerDependencies {
        for (name, ver) in deps_map {
            deps.push(DependencyEntry {
                name,
                source: "npm".to_string(),
                kind: Some("peer".to_string()),
                version: Some(ver),
                repo: None,
                url: None,
                commit: None,
                path: None,
            });
        }
    }

    if let Some(deps_map) = raw.optionalDependencies {
        for (name, ver) in deps_map {
            deps.push(DependencyEntry {
                name,
                source: "npm".to_string(),
                kind: Some("optional".to_string()),
                version: Some(ver),
                repo: None,
                url: None,
                commit: None,
                path: None,
            });
        }
    }

    let workspace = raw.workspaces.map(|members| Workspace { members });

    let scripts = raw.scripts.map_or_else(Vec::new, |map| {
        map.into_iter()
            .map(|(name, command)| ScriptEntry { name, command })
            .collect()
    });

    Ok(Manifest {
        project,
        deps,
        workspace: workspace.map(|w| crate::manifest::types::Workspace { members: w.members }),
        scripts,
        security: None,
        build: None,
        package_json_extras: Some(content.to_string()),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_parse_minimal() {
        let json = r#"{"name": "my-app", "version": "0.1.0"}"#;
        let m = parse_package_json(json).unwrap();
        assert_eq!(m.project.name, "my-app");
        assert_eq!(m.project.version, "0.1.0");
        assert!(m.deps.is_empty());
        assert!(m.workspace.is_none());
        assert!(m.scripts.is_empty());
    }

    #[test]
    fn test_parse_with_dependencies() {
        let json = r#"{
            "name": "app",
            "version": "1.0.0",
            "dependencies": {
                "zod": "^3.0.0",
                "react": "18.2.0"
            },
            "devDependencies": {
                "vitest": "^1.0.0",
                "typescript": "^5.0.0"
            },
            "peerDependencies": {
                "react-dom": "^18.0.0"
            },
            "optionalDependencies": {
                "fsevents": "^2.0.0"
            }
        }"#;
        let m = parse_package_json(json).unwrap();
        assert_eq!(m.deps.len(), 6);

        let zod = m.deps.iter().find(|d| d.name == "zod").unwrap();
        assert_eq!(zod.source, "npm");
        assert_eq!(zod.kind.as_deref(), Some("prod"));
        assert_eq!(zod.version.as_deref(), Some("^3.0.0"));

        let vitest = m.deps.iter().find(|d| d.name == "vitest").unwrap();
        assert_eq!(vitest.kind.as_deref(), Some("dev"));

        let react_dom = m.deps.iter().find(|d| d.name == "react-dom").unwrap();
        assert_eq!(react_dom.kind.as_deref(), Some("peer"));

        let fsevents = m.deps.iter().find(|d| d.name == "fsevents").unwrap();
        assert_eq!(fsevents.kind.as_deref(), Some("optional"));
    }

    #[test]
    fn test_parse_scoped_packages() {
        let json = r#"{
            "name": "my-app",
            "version": "1.0.0",
            "dependencies": {
                "@angular/core": "^17.0.0",
                "@angular/common": "^17.0.0"
            }
        }"#;
        let m = parse_package_json(json).unwrap();
        assert_eq!(m.deps.len(), 2);
        assert!(m.deps.iter().any(|d| d.name == "@angular/core"));
        assert!(m.deps.iter().any(|d| d.name == "@angular/common"));
    }

    #[test]
    fn test_parse_with_scripts() {
        let json = r#"{
            "name": "app",
            "version": "0.1.0",
            "scripts": {
                "build": "tsc",
                "test": "vitest run",
                "start": "node dist/index.js"
            }
        }"#;
        let m = parse_package_json(json).unwrap();
        assert_eq!(m.scripts.len(), 3);
        let build = m.scripts.iter().find(|s| s.name == "build").unwrap();
        assert_eq!(build.command, "tsc");
    }

    #[test]
    fn test_parse_with_workspaces() {
        let json = r#"{
            "name": "monorepo",
            "version": "0.1.0",
            "workspaces": ["apps/*", "packages/*"]
        }"#;
        let m = parse_package_json(json).unwrap();
        assert!(m.workspace.is_some());
        let ws = m.workspace.unwrap();
        assert_eq!(ws.members.len(), 2);
        assert_eq!(ws.members[0], "apps/*");
    }

    #[test]
    fn test_parse_missing_name() {
        let json = r#"{"version": "1.0.0"}"#;
        match parse_package_json(json) {
            Err(ManifestParseError::MissingProjectName) => {}
            other => panic!("expected MissingProjectName, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_missing_version() {
        let json = r#"{"name": "app"}"#;
        match parse_package_json(json) {
            Err(ManifestParseError::MissingProjectVersion) => {}
            other => panic!("expected MissingProjectVersion, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_invalid_json() {
        let json = r#"not valid json"#;
        match parse_package_json(json) {
            Err(ManifestParseError::Json(_)) => {}
            other => panic!("expected Json error, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_preserves_raw_json() {
        let json =
            r#"{"name": "app", "version": "1.0.0", "main": "dist/index.js", "private": true}"#;
        let m = parse_package_json(json).unwrap();
        assert!(m.package_json_extras.is_some());
        let saved = m.package_json_extras.unwrap();
        assert!(saved.contains("main"));
        assert!(saved.contains("private"));
        assert!(saved.contains("dist/index.js"));
    }

    #[test]
    fn test_parse_repository_as_string() {
        let json = r#"{"name": "app", "version": "1.0.0", "repository": "user/repo"}"#;
        let m = parse_package_json(json).unwrap();
        assert_eq!(m.project.name, "app");
        assert!(m.package_json_extras.is_some());
        let saved = m.package_json_extras.unwrap();
        assert!(saved.contains("user/repo"));
    }

    #[test]
    fn test_parse_repository_as_object() {
        let json = r#"{
            "name": "app",
            "version": "1.0.0",
            "repository": {
                "type": "git",
                "url": "git+https://github.com/user/repo.git"
            }
        }"#;
        let m = parse_package_json(json).unwrap();
        assert!(m.package_json_extras.is_some());
    }

    #[test]
    fn test_parse_empty_deps() {
        let json = r#"{
            "name": "app",
            "version": "1.0.0",
            "dependencies": {},
            "devDependencies": {}
        }"#;
        let m = parse_package_json(json).unwrap();
        assert!(m.deps.is_empty());
    }

    #[test]
    fn test_parse_with_unicode() {
        let json = r#"{"name": "🔥-test-中文", "version": "1.0.0"}"#;
        let m = parse_package_json(json).unwrap();
        assert_eq!(m.project.name, "🔥-test-中文");
    }
}
