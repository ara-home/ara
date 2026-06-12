//! Package source implementations: local directories, workspace members,
//! git repositories, GitHub archives, and npm/registry tarballs.

pub mod git;
pub mod github;
pub mod local;
pub mod registry;
pub mod tarball;
pub mod url;
pub mod workspace;

use ara_types::PackageIdentity;

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
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub enum Source {
    Local(local::LocalSource),
    Workspace(workspace::WorkspaceSource),
    Git(git::GitSource),
    Github(github::GithubSource),
    Registry(registry::RegistrySource),
    Url(tarball::TarballSource),
}

impl Source {
    pub async fn resolve(&self, name: &str) -> Result<String, SourceError> {
        match self {
            Self::Local(s) => s.resolve(name).await,
            Self::Workspace(s) => s.resolve(name).await,
            Self::Git(s) => s.resolve(name).await,
            Self::Github(s) => s.resolve(name).await,
            Self::Registry(s) => s.resolve(name).await,
            Self::Url(s) => s.resolve(name).await,
        }
    }

    pub async fn fetch(&self, identity: &PackageIdentity) -> Result<Vec<u8>, SourceError> {
        match self {
            Self::Local(s) => s.fetch(identity).await,
            Self::Workspace(s) => s.fetch(identity).await,
            Self::Git(s) => s.fetch(identity).await,
            Self::Github(s) => s.fetch(identity).await,
            Self::Registry(s) => s.fetch(identity).await,
            Self::Url(s) => s.fetch(identity).await,
        }
    }
}
