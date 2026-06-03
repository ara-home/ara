pub mod git;
pub mod github;
pub mod local;
pub mod registry;
pub mod workspace;

use crate::types::PackageIdentity;

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("package not found")]
    PackageNotFound,
    #[error("version not found")]
    VersionNotFound,
    #[error("fetch failed: {0}")]
    FetchFailed(String),
    #[error("invalid source configuration")]
    InvalidSource,
    #[error("git error: {0}")]
    GitError(String),
    #[error("network error: {0}")]
    NetworkError(String),
    #[error("integrity mismatch")]
    IntegrityMismatch,
    #[error("tar error: {0}")]
    TarError(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct ResolveResult {
    pub name: String,
    pub version: String,
    pub package_hash: String,
}

pub enum Source {
    Local(local::LocalSource),
    Workspace(workspace::WorkspaceSource),
    Git(git::GitSource),
    Github(github::GithubSource),
    Registry(registry::RegistrySource),
    Npm(registry::RegistrySource),
}

impl Source {
    pub fn resolve(&self, name: &str) -> Result<String, SourceError> {
        match self {
            Self::Local(s) => s.resolve(name),
            Self::Workspace(s) => s.resolve(name),
            Self::Git(s) => s.resolve(name),
            Self::Github(s) => s.resolve(name),
            Self::Registry(s) | Self::Npm(s) => s.resolve(name),
        }
    }

    pub fn fetch(&self, identity: &PackageIdentity) -> Result<Vec<u8>, SourceError> {
        match self {
            Self::Local(s) => s.fetch(identity),
            Self::Workspace(s) => s.fetch(identity),
            Self::Git(s) => s.fetch(identity),
            Self::Github(s) => s.fetch(identity),
            Self::Registry(s) | Self::Npm(s) => s.fetch(identity),
        }
    }
}
