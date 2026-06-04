//! Package source implementations: local directories, workspace members,
//! git repositories, GitHub archives, and npm/registry tarballs.

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
    #[error("git error: {0}")]
    GitError(String),
    #[error("network error: {0}")]
    NetworkError(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub enum Source {
    Local(local::LocalSource),
    Workspace(workspace::WorkspaceSource),
    Git(git::GitSource),
    Github(github::GithubSource),
    Registry(registry::RegistrySource),
}

impl Source {
    pub fn resolve(&self, name: &str) -> Result<String, SourceError> {
        match self {
            Self::Local(s) => s.resolve(name),
            Self::Workspace(s) => s.resolve(name),
            Self::Git(s) => s.resolve(name),
            Self::Github(s) => s.resolve(name),
            Self::Registry(s) => s.resolve(name),
        }
    }

    pub fn fetch(&self, identity: &PackageIdentity) -> Result<Vec<u8>, SourceError> {
        match self {
            Self::Local(s) => s.fetch(identity),
            Self::Workspace(s) => s.fetch(identity),
            Self::Git(s) => s.fetch(identity),
            Self::Github(s) => s.fetch(identity),
            Self::Registry(s) => s.fetch(identity),
        }
    }
}
