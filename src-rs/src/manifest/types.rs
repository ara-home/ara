#![allow(dead_code)]

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Project {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DepEntryRaw {
    pub source: Option<String>,
    pub version: Option<String>,
    pub repo: Option<String>,
    pub url: Option<String>,
    pub commit: Option<String>,
    pub path: Option<String>,
    #[serde(rename = "package")]
    pub package: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DependencyEntry {
    pub name: String,
    pub source: String,
    pub version: Option<String>,
    pub repo: Option<String>,
    pub url: Option<String>,
    pub commit: Option<String>,
    pub path: Option<String>,
    pub package: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub members: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ScriptEntry {
    pub name: String,
    pub command: String,
}

#[derive(Debug, Clone, Default)]
pub struct Security {
    pub risk_threshold: Option<String>,
    pub require_review: Option<bool>,
    pub allow_lifecycle_scripts: Option<bool>,
    pub block_critical: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct Build {
    pub hermetic: Option<bool>,
    pub offline_first: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub project: Project,
    pub deps: Vec<DependencyEntry>,
    pub workspace: Option<Workspace>,
    pub scripts: Vec<ScriptEntry>,
    pub security: Option<Security>,
    pub build: Option<Build>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_project_creation() {
        let p = Project {
            name: "test".into(),
            version: "0.1.0".into(),
            description: Some("desc".into()),
            license: None,
            repository: None,
            homepage: None,
        };
        assert_eq!(p.name, "test");
        assert_eq!(p.version, "0.1.0");
    }

    #[test]
    fn test_dependency_entry_creation() {
        let d = DependencyEntry {
            name: "zod".into(),
            source: "npm".into(),
            version: Some("^3.0.0".into()),
            repo: None,
            url: None,
            commit: None,
            path: None,
            package: None,
        };
        assert_eq!(d.name, "zod");
        assert_eq!(d.version.as_deref(), Some("^3.0.0"));
    }

    #[test]
    fn test_manifest_with_all_sections() {
        let m = Manifest {
            project: Project {
                name: "app".into(),
                version: "1.0.0".into(),
                description: None,
                license: None,
                repository: None,
                homepage: None,
            },
            deps: vec![DependencyEntry {
                name: "react".into(),
                source: "npm".into(),
                version: Some("^18.0.0".into()),
                repo: None,
                url: None,
                commit: None,
                path: None,
                package: None,
            }],
            workspace: Some(Workspace { members: vec!["apps/*".into()] }),
            scripts: vec![ScriptEntry { name: "build".into(), command: "tsc".into() }],
            security: Some(Security {
                risk_threshold: Some("high".into()),
                require_review: Some(true),
                allow_lifecycle_scripts: None,
                block_critical: None,
            }),
            build: Some(Build {
                hermetic: Some(true),
                offline_first: None,
            }),
        };
        assert_eq!(m.project.name, "app");
        assert_eq!(m.deps.len(), 1);
        assert_eq!(m.workspace.as_ref().unwrap().members.len(), 1);
        assert_eq!(m.scripts.len(), 1);
        assert_eq!(m.security.as_ref().unwrap().risk_threshold.as_deref(), Some("high"));
        assert!(m.build.as_ref().unwrap().hermetic.unwrap());
    }

    #[test]
    fn test_security_default() {
        let s = Security::default();
        assert!(s.risk_threshold.is_none());
        assert!(s.require_review.is_none());
    }

    #[test]
    fn test_build_default() {
        let b = Build::default();
        assert!(b.hermetic.is_none());
        assert!(b.offline_first.is_none());
    }
}
