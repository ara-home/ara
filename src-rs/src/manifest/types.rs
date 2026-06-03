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
