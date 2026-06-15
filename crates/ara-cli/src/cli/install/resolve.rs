use std::sync::Arc;

use anyhow::{Context, Result};

use ara_source::Source;
use ara_store::cas::Store;
use ara_store::index::StoreIndex;
use ara_types::{PackageIdentity, SourceType, Version};

pub(crate) fn create_source(
    source_type: SourceType,
    dep: &ara_manifest::types::DependencyEntry,
) -> Result<Source> {
    Ok(match source_type {
        SourceType::Npm | SourceType::Registry => {
            let default_url = std::env::var("ARA_NPM_REGISTRY")
                .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());
            let url = dep.url.as_deref().unwrap_or(&default_url);
            Source::Registry(ara_source::registry::RegistrySource::new(url.to_string())?)
        }
        SourceType::Github => {
            let repo = dep
                .repo
                .as_deref()
                .context("missing repo for github source")?;
            Source::Github(ara_source::github::GithubSource::new(repo.to_string()))
        }
        SourceType::Git => {
            let url = dep.url.as_deref().context("missing url for git source")?;
            let commit = dep.commit.as_deref().unwrap_or("HEAD");
            Source::Git(ara_source::git::GitSource::new(
                url.to_string(),
                commit.to_string(),
            ))
        }
        SourceType::Local => {
            let path = dep
                .path
                .as_deref()
                .context("missing path for local source")?;
            Source::Local(ara_source::local::LocalSource::new(path.to_string()))
        }
        SourceType::Url => {
            let url = dep.url.as_deref().context("missing url for url source")?;
            Source::Url(ara_source::tarball::TarballSource::new(url.to_string()))
        }
        SourceType::Workspace => {
            let path = dep.path.as_deref().unwrap_or(".");
            Source::Workspace(ara_source::workspace::WorkspaceSource::new(
                path.to_string(),
            ))
        }
    })
}

/// Resolved package metadata (without content).
pub(crate) struct ResolvedMeta {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) version_semver: Version,
    pub(crate) source_type: SourceType,
    pub(crate) source: String,
    pub(crate) url: Option<String>,
    pub(crate) repo: Option<String>,
    pub(crate) commit: Option<String>,
    pub(crate) integrity: Option<String>,
}

/// Fetch content for a resolved meta, returning raw bytes.
pub(crate) async fn fetch_meta_content(meta: &ResolvedMeta) -> Result<Vec<u8>> {
    match meta.source_type {
        SourceType::Npm | SourceType::Registry => {
            let registry_url = std::env::var("ARA_NPM_REGISTRY")
                .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());
            let reg = ara_source::registry::RegistrySource::new(registry_url)?;
            let identity = PackageIdentity {
                source: SourceType::Npm,
                name: meta.name.clone(),
                version: meta.version_semver.clone(),
                content_hash: meta.integrity.clone(),
                requested_ref: None,
            };
            reg.fetch(&identity)
                .await
                .with_context(|| format!("failed to fetch {}@{}", meta.name, meta.version))
        }
        SourceType::Github => {
            let repo = meta.repo.as_deref().unwrap_or(&meta.name);
            let src = ara_source::github::GithubSource::new(repo.to_string());
            let identity = PackageIdentity {
                source: SourceType::Github,
                name: repo.to_string(),
                version: meta.version_semver.clone(),
                content_hash: None,
                requested_ref: meta.commit.clone(),
            };
            src.fetch(&identity)
                .await
                .with_context(|| format!("failed to fetch github:{repo}"))
        }
        SourceType::Git => {
            let url = meta.url.as_deref().unwrap_or(&meta.name);
            let commit_str = meta.commit.clone().unwrap_or_else(|| "HEAD".to_string());
            let src = ara_source::git::GitSource::new(url.to_string(), commit_str.clone());
            let identity = PackageIdentity {
                source: SourceType::Git,
                name: url.to_string(),
                version: meta.version_semver.clone(),
                content_hash: None,
                requested_ref: Some(commit_str),
            };
            src.fetch(&identity)
                .await
                .with_context(|| format!("failed to fetch git:{url}"))
        }
        SourceType::Url => {
            let url = meta.url.as_deref().unwrap_or(&meta.name);
            let src = ara_source::tarball::TarballSource::new(url.to_string());
            let identity = PackageIdentity {
                source: SourceType::Url,
                name: url.to_string(),
                version: Version::new(0, 0, 0),
                content_hash: None,
                requested_ref: None,
            };
            src.fetch(&identity)
                .await
                .with_context(|| format!("failed to download {url}"))
        }
        _ => anyhow::bail!("unsupported source type: {}", meta.source_type),
    }
}

async fn resolve_npm_meta(
    name: &str,
    version: Option<&str>,
    range: Option<&str>,
) -> Result<ResolvedMeta> {
    let registry_url = std::env::var("ARA_NPM_REGISTRY")
        .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());

    let reg = ara_source::registry::RegistrySource::new(registry_url)?;

    let (resolved_ver_str, manifest_ver) = if let Some(v) = version {
        let trimmed = v
            .trim_start_matches('^')
            .trim_start_matches('~')
            .trim_start_matches('>')
            .trim_start_matches('<')
            .trim_start_matches('=');
        let is_exact = v == trimmed;
        if is_exact {
            Version::parse(v).with_context(|| format!("invalid version: {v}"))?;
            (v.to_string(), v.to_string())
        } else {
            let concrete = reg
                .resolve(name)
                .await
                .with_context(|| format!("failed to resolve {name} for range {v}"))?;
            (concrete, v.to_string())
        }
    } else {
        let concrete = reg
            .resolve(name)
            .await
            .with_context(|| format!("failed to resolve {name}"))?;
        let manifest = apply_range(&concrete, range);
        (concrete, manifest)
    };

    let parsed_ver = Version::parse(&resolved_ver_str)
        .with_context(|| format!("invalid version from registry: {resolved_ver_str}"))?;

    // Fetch metadata to extract integrity hash for the resolved version
    let integrity = reg.fetch_metadata(name).await.ok().and_then(|meta| {
        let ver_data = meta["versions"][&resolved_ver_str].as_object()?;
        let dist = ver_data.get("dist")?;
        dist["integrity"]
            .as_str()
            .or_else(|| dist["shasum"].as_str())
            .map(|s| s.to_string())
    });

    Ok(ResolvedMeta {
        name: name.to_string(),
        version: manifest_ver,
        version_semver: parsed_ver,
        source_type: SourceType::Npm,
        source: "npm".to_string(),
        url: None,
        repo: None,
        commit: None,
        integrity,
    })
}

fn resolve_github_meta(repo: &str, commit: Option<&str>) -> Result<ResolvedMeta> {
    let ver_str = commit.unwrap_or("HEAD").to_string();
    Ok(ResolvedMeta {
        name: repo.to_string(),
        version: ver_str,
        version_semver: Version::new(0, 0, 0),
        source_type: SourceType::Github,
        source: "github".to_string(),
        url: None,
        repo: Some(repo.to_string()),
        commit: commit.map(|c| c.to_string()),
        integrity: None,
    })
}

fn resolve_git_meta(url: &str, commit: Option<&str>) -> Result<ResolvedMeta> {
    let commit_str = commit.unwrap_or("HEAD").to_string();
    let name = derive_name_from_git_url(url)
        .unwrap_or_else(|| url.rsplit('/').next().unwrap_or(url).to_string());
    Ok(ResolvedMeta {
        name,
        version: commit_str.clone(),
        version_semver: Version::new(0, 0, 0),
        source_type: SourceType::Git,
        source: "git".to_string(),
        url: Some(url.to_string()),
        repo: None,
        commit: Some(commit_str),
        integrity: None,
    })
}

fn resolve_tarball_meta(url: &str) -> Result<ResolvedMeta> {
    // Tarball identity is unknown until download; name/version filled after fetch.
    Ok(ResolvedMeta {
        name: String::new(),
        version: String::new(),
        version_semver: Version::new(0, 0, 0),
        source_type: SourceType::Url,
        source: "url".to_string(),
        url: Some(url.to_string()),
        repo: None,
        commit: None,
        integrity: None,
    })
}

pub(crate) async fn resolve_spec_meta(
    target: &ara_source::url::InstallTarget,
    range: Option<&str>,
) -> Result<ResolvedMeta> {
    match target {
        ara_source::url::InstallTarget::Npm { name, version } => {
            resolve_npm_meta(name, version.as_deref(), range).await
        }
        ara_source::url::InstallTarget::Github { repo, commit } => {
            resolve_github_meta(repo, commit.as_deref())
        }
        ara_source::url::InstallTarget::Git { url, commit } => {
            resolve_git_meta(url, commit.as_deref())
        }
        ara_source::url::InstallTarget::Tarball { url } => resolve_tarball_meta(url),
    }
}

fn apply_range(version: &str, range: Option<&str>) -> String {
    match range {
        Some("caret") => format!("^{version}"),
        Some("patch") => format!("~{version}"),
        _ => version.to_string(),
    }
}

fn derive_name_from_git_url(url: &str) -> Option<String> {
    // https://github.com/user/repo.git → "repo"
    // git@github.com:user/repo.git → "repo"
    // https://bitbucket.org/user/repo → "repo"
    let without_fragment = url.split('#').next().unwrap_or(url);
    let without_git = without_fragment
        .strip_suffix(".git")
        .unwrap_or(without_fragment);
    let name = without_git.rsplit('/').next()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

pub(crate) async fn fetch_and_store_parallel(
    store: &Store,
    store_index: &Arc<StoreIndex>,
    src: &Source,
    cache_key: &str,
    node: &ara_resolver::graph::Node,
    ver_str: &str,
) -> Option<(String, Vec<u8>)> {
    println!("  fetching {}@{}...", node.name, ver_str);

    let identity = PackageIdentity {
        source: node.source,
        name: node.name.clone(),
        version: node.version.clone(),
        content_hash: None,
        requested_ref: None,
    };

    let pkg_content = match src.fetch(&identity).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("  failed to fetch {}: {}", node.name, e);
            return None;
        }
    };

    let hash_str = match store.put(&pkg_content) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("  failed to store {}: {}", node.name, e);
            return None;
        }
    };

    if let Err(e) = store_index.insert(
        cache_key,
        &hash_str,
        &node.source.to_string(),
        pkg_content.len() as i64,
    ) {
        eprintln!(
            "  warning: failed to index fetch result for {}: {}",
            node.name, e
        );
    }

    Some((hash_str, pkg_content))
}
