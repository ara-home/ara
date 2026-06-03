use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct GraphMeta {
    #[serde(default = "default_resolver")]
    pub resolver: String,
    pub generated_at: Option<String>,
    pub graph_hash: Option<String>,
}

fn default_resolver() -> String {
    "mvs".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityMeta {
    pub risk_level: Option<String>,
    pub analysis_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SbomMeta {
    pub license: Option<String>,
    pub supplier: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PackageEntry {
    pub name: String,
    pub version: String,
    pub source: String,
    pub package_hash: String,
    pub integrity: Option<String>,
    pub signature: Option<String>,
    pub repository: Option<String>,
    pub commit: Option<String>,
    pub dependencies: Option<Vec<String>>,
    pub security: Option<SecurityMeta>,
    pub sbom: Option<SbomMeta>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Lockfile {
    #[serde(default)]
    pub version: u32,
    #[serde(default = "default_graph_meta")]
    pub graph: GraphMeta,
    #[serde(default)]
    #[serde(rename = "package")]
    pub packages: Vec<PackageEntry>,
}

fn default_graph_meta() -> GraphMeta {
    GraphMeta {
        resolver: "mvs".to_string(),
        generated_at: None,
        graph_hash: None,
    }
}
