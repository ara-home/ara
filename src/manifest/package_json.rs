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

#[allow(dead_code)]
pub fn generate_package_json(manifest: &Manifest) -> String {
    let mut map = serde_json::Map::new();

    map.insert(
        "name".to_string(),
        serde_json::Value::String(manifest.project.name.clone()),
    );
    map.insert(
        "version".to_string(),
        serde_json::Value::String(manifest.project.version.clone()),
    );

    // Group deps by kind
    let mut prod_deps = BTreeMap::new();
    let mut dev_deps = BTreeMap::new();
    let mut peer_deps = BTreeMap::new();
    let mut optional_deps = BTreeMap::new();

    for dep in &manifest.deps {
        let ver = dep.version.clone().unwrap_or_else(|| "*".to_string());
        match dep.kind.as_deref() {
            Some("dev") => {
                dev_deps.insert(dep.name.clone(), ver);
            }
            Some("peer") => {
                peer_deps.insert(dep.name.clone(), ver);
            }
            Some("optional") => {
                optional_deps.insert(dep.name.clone(), ver);
            }
            _ => {
                prod_deps.insert(dep.name.clone(), ver);
            }
        }
    }

    if !prod_deps.is_empty() {
        map.insert(
            "dependencies".to_string(),
            serde_json::Value::Object(
                prod_deps
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::String(v)))
                    .collect(),
            ),
        );
    }
    if !dev_deps.is_empty() {
        map.insert(
            "devDependencies".to_string(),
            serde_json::Value::Object(
                dev_deps
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::String(v)))
                    .collect(),
            ),
        );
    }
    if !peer_deps.is_empty() {
        map.insert(
            "peerDependencies".to_string(),
            serde_json::Value::Object(
                peer_deps
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::String(v)))
                    .collect(),
            ),
        );
    }
    if !optional_deps.is_empty() {
        map.insert(
            "optionalDependencies".to_string(),
            serde_json::Value::Object(
                optional_deps
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::String(v)))
                    .collect(),
            ),
        );
    }

    if !manifest.scripts.is_empty() {
        let scripts: BTreeMap<String, String> = manifest
            .scripts
            .iter()
            .map(|s| (s.name.clone(), s.command.clone()))
            .collect();
        map.insert(
            "scripts".to_string(),
            serde_json::Value::Object(
                scripts
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::String(v)))
                    .collect(),
            ),
        );
    }

    if let Some(ws) = &manifest.workspace {
        let members: Vec<serde_json::Value> = ws
            .members
            .iter()
            .map(|m| serde_json::Value::String(m.clone()))
            .collect();
        map.insert("workspaces".to_string(), serde_json::Value::Array(members));
    }

    // Merge extras (preserved fields from original package.json)
    if let Some(extras_json) = &manifest.package_json_extras {
        if let Ok(extras) =
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(extras_json)
        {
            let mapped_keys = [
                "name",
                "version",
                "dependencies",
                "devDependencies",
                "peerDependencies",
                "optionalDependencies",
                "scripts",
                "workspaces",
            ];
            for (k, v) in extras {
                if !mapped_keys.contains(&k.as_str()) {
                    map.insert(k, v);
                }
            }
        }
    }

    let root = serde_json::Value::Object(map);
    serde_json::to_string_pretty(&root).unwrap_or_else(|_| "{}".to_string())
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

    // -- generator tests --

    #[test]
    fn test_generate_minimal() {
        let m = Manifest {
            project: Project {
                name: "app".into(),
                version: "1.0.0".into(),
            },
            deps: vec![],
            workspace: None,
            scripts: vec![],
            security: None,
            build: None,
            package_json_extras: None,
        };
        let out = generate_package_json(&m);
        assert!(out.contains(r#""app""#));
        assert!(out.contains(r#""1.0.0""#));
    }

    #[test]
    fn test_generate_with_deps() {
        let m = Manifest {
            project: Project {
                name: "app".into(),
                version: "0.1.0".into(),
            },
            deps: vec![
                DependencyEntry {
                    name: "zod".into(),
                    source: "npm".into(),
                    kind: Some("prod".into()),
                    version: Some("^3.0.0".into()),
                    repo: None,
                    url: None,
                    commit: None,
                    path: None,
                },
                DependencyEntry {
                    name: "vitest".into(),
                    source: "npm".into(),
                    kind: Some("dev".into()),
                    version: Some("^1.0.0".into()),
                    repo: None,
                    url: None,
                    commit: None,
                    path: None,
                },
            ],
            workspace: None,
            scripts: vec![],
            security: None,
            build: None,
            package_json_extras: None,
        };
        let out = generate_package_json(&m);
        assert!(out.contains(r#""dependencies""#));
        assert!(out.contains(r#""zod""#));
        assert!(out.contains(r#""devDependencies""#));
        assert!(out.contains(r#""vitest""#));
        assert!(out.contains(r#""^3.0.0""#));
        assert!(out.contains(r#""^1.0.0""#));
    }

    #[test]
    fn test_generate_with_scripts() {
        let m = Manifest {
            project: Project {
                name: "app".into(),
                version: "0.1.0".into(),
            },
            deps: vec![],
            workspace: None,
            scripts: vec![ScriptEntry {
                name: "build".into(),
                command: "tsc".into(),
            }],
            security: None,
            build: None,
            package_json_extras: None,
        };
        let out = generate_package_json(&m);
        assert!(out.contains(r#""scripts""#));
        assert!(out.contains(r#""build""#));
        assert!(out.contains(r#"tsc"#));
    }

    #[test]
    fn test_generate_roundtrip() {
        let json = r#"{
            "name": "my-app",
            "version": "0.1.0",
            "description": "A test app",
            "private": true,
            "main": "dist/index.js",
            "dependencies": {
                "zod": "^3.0.0"
            },
            "devDependencies": {
                "vitest": "^1.0.0"
            },
            "scripts": {
                "build": "tsc"
            }
        }"#;
        let m = parse_package_json(json).unwrap();
        let generated = generate_package_json(&m);

        // Original extras are preserved
        assert!(generated.contains(r#""description""#));
        assert!(generated.contains(r#"A test app"#));
        assert!(generated.contains(r#""private""#));
        assert!(generated.contains(r#""main""#));
        assert!(generated.contains(r#"dist/index.js"#));

        // Mapped fields are present
        assert!(generated.contains(r#""dependencies""#));
        assert!(generated.contains(r#""devDependencies""#));
        assert!(generated.contains(r#""scripts""#));
        assert!(generated.contains(r#""zod""#));
        assert!(generated.contains(r#""vitest""#));
        assert!(generated.contains(r#""build""#));

        // Result is valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&generated).unwrap();
        let obj = parsed.as_object().unwrap();
        assert_eq!(obj["name"].as_str(), Some("my-app"));
        assert_eq!(obj["version"].as_str(), Some("0.1.0"));
        assert_eq!(obj["description"].as_str(), Some("A test app"));
        assert_eq!(obj["private"].as_bool(), Some(true));
        assert_eq!(obj["main"].as_str(), Some("dist/index.js"));
    }

    #[test]
    fn test_generate_with_workspaces() {
        let m = Manifest {
            project: Project {
                name: "monorepo".into(),
                version: "0.1.0".into(),
            },
            deps: vec![],
            workspace: Some(Workspace {
                members: vec!["apps/*".into(), "packages/*".into()],
            }),
            scripts: vec![],
            security: None,
            build: None,
            package_json_extras: None,
        };
        let out = generate_package_json(&m);
        assert!(out.contains(r#""workspaces""#));
        assert!(out.contains(r#"apps/*"#));
        assert!(out.contains(r#"packages/*"#));

        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let ws = parsed["workspaces"].as_array().unwrap();
        assert_eq!(ws.len(), 2);
        assert_eq!(ws[0].as_str(), Some("apps/*"));
    }
}
