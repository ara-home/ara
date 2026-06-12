use std::fmt;

/// The result of parsing a package install spec string.
#[derive(Debug, Clone, PartialEq)]
pub enum InstallTarget {
    /// Plain npm registry package: "react", "react@18.2.0", "react@^18", "@scope/name"
    Npm {
        name: String,
        version: Option<String>,
    },
    /// GitHub shorthand: "user/repo", "user/repo#abc123", "user/repo#develop"
    Github {
        repo: String,
        commit: Option<String>,
    },
    /// Git URL: "https://github.com/user/repo.git", "git+ssh://..." with optional #ref
    Git { url: String, commit: Option<String> },
    /// Direct tarball URL: "https://example.com/pkg.tgz", "./local.tar.gz"
    Tarball { url: String },
}

impl fmt::Display for InstallTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Npm { name, version } => {
                write!(f, "{name}")?;
                if let Some(v) = version {
                    write!(f, "@{v}")?;
                }
                Ok(())
            }
            Self::Github { repo, commit } => {
                write!(f, "{repo}")?;
                if let Some(c) = commit {
                    write!(f, "#{c}")?;
                }
                Ok(())
            }
            Self::Git { url, commit } => {
                write!(f, "{url}")?;
                if let Some(c) = commit {
                    write!(f, "#{c}")?;
                }
                Ok(())
            }
            Self::Tarball { url } => write!(f, "{url}"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ParseSpecError {
    #[error("empty spec")]
    Empty,
    #[error("unknown spec format: '{0}'")]
    UnknownFormat(String),
}

/// Parse a package install spec string into an [`InstallTarget`].
///
/// # Supported formats
///
/// | Input | Target | Example |
/// |---|---|---|
/// | Plain name | `Npm` | `react` |
/// | Name + version | `Npm` | `react@18.2.0` |
/// | Name + range | `Npm` | `react@^18`, `react@~18.2` |
/// | Scoped package | `Npm` | `@angular/core@17.0.0` |
/// | GitHub shorthand | `Github` | `user/repo` |
/// | GitHub + ref | `Github` | `user/repo#abc123` |
/// | Git URL | `Git` | `https://github.com/user/repo.git` |
/// | Git URL + ref | `Git` | `https://github.com/user/repo.git#v1.0` |
/// | Explicit git scheme | `Git` | `git+https://github.com/user/repo.git` |
/// | Tarball URL | `Tarball` | `https://example.com/pkg.tgz` |
/// | Local tarball | `Tarball` | `./local.tar.gz` |
///
/// # Ambiguity rules
///
/// - Protocol presence (`://`, `git+`) → URL-based (Git or Tarball)
/// - `.tgz` / `.tar.gz` suffix → `Tarball`, regardless of protocol
/// - `.git` suffix → `Git`
/// - Starts with `@` → npm scoped package
/// - Contains `/` but no protocol → GitHub shorthand
/// - Contains `@` → npm with version
/// - Everything else → npm plain name
pub fn parse_install_spec(spec: &str) -> Result<InstallTarget, ParseSpecError> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err(ParseSpecError::Empty);
    }

    // Has protocol → URL-based (Git or Tarball)
    if spec.contains("://") || spec.starts_with("git+") {
        return parse_url_spec(spec);
    }

    // Local file path ending in .tgz or .tar.gz
    if spec.ends_with(".tgz") || spec.ends_with(".tar.gz") {
        return Ok(InstallTarget::Tarball {
            url: spec.to_string(),
        });
    }

    // Explicit .git suffix (even without protocol): "bitbucket.org/user/repo.git"
    // or with fragment: "bitbucket.org/user/repo.git#v1.0"
    let base = spec.split('#').next().unwrap_or(spec);
    if base.ends_with(".git") && spec.contains('/') {
        let (url_part, commit) = split_fragment(spec);
        return Ok(InstallTarget::Git {
            url: url_part.to_string(),
            commit,
        });
    }

    // SCP-style SSH git URL: contains @ followed by host:path
    // e.g. "git@github.com:user/repo.git"
    if contains_at_syntax(spec) {
        let (url_part, commit) = split_fragment(spec);
        return Ok(InstallTarget::Git {
            url: url_part.to_string(),
            commit,
        });
    }

    // Starts with @ → npm scoped package
    if spec.starts_with('@') {
        return parse_npm_scoped(spec);
    }

    // Contains '/' → GitHub shorthand (npm names cannot contain '/')
    if spec.contains('/') {
        return parse_github_shorthand(spec);
    }

    // Contains '@' → npm with version
    if spec.contains('@') {
        return parse_npm_with_version(spec);
    }

    // Plain name → npm
    Ok(InstallTarget::Npm {
        name: spec.to_string(),
        version: None,
    })
}

fn parse_url_spec(spec: &str) -> Result<InstallTarget, ParseSpecError> {
    let url = spec.strip_prefix("git+").unwrap_or(spec);

    let (url_part, commit) = split_fragment(url);

    // Tarball: ends with .tgz or .tar.gz
    if url_part.ends_with(".tgz") || url_part.ends_with(".tar.gz") {
        return Ok(InstallTarget::Tarball {
            url: url_part.to_string(),
        });
    }

    // Git URL: ends with .git, or any other URL with a protocol
    Ok(InstallTarget::Git {
        url: url_part.to_string(),
        commit,
    })
}

fn parse_npm_scoped(spec: &str) -> Result<InstallTarget, ParseSpecError> {
    // spec looks like "@scope/name" or "@scope/name@version"
    // We need to find the @version separator, which is the LAST '@' after position 0
    if let Some(at_pos) = spec[1..].rfind('@') {
        // at_pos is relative to spec[1..], so in spec terms it's at at_pos + 1
        let sep = at_pos + 1; // position of '@' in spec
        if sep > 1 && sep < spec.len() - 1 {
            let name = &spec[..sep];
            let version = &spec[sep + 1..];
            return Ok(InstallTarget::Npm {
                name: name.to_string(),
                version: Some(version.to_string()),
            });
        }
    }

    Ok(InstallTarget::Npm {
        name: spec.to_string(),
        version: None,
    })
}

fn parse_github_shorthand(spec: &str) -> Result<InstallTarget, ParseSpecError> {
    let (repo_part, commit) = split_fragment(spec);

    let parts: Vec<&str> = repo_part.split('/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(ParseSpecError::UnknownFormat(spec.to_string()));
    }

    Ok(InstallTarget::Github {
        repo: repo_part.to_string(),
        commit,
    })
}

fn parse_npm_with_version(spec: &str) -> Result<InstallTarget, ParseSpecError> {
    let at_pos = spec.rfind('@').unwrap_or(0);
    if at_pos == 0 {
        return Err(ParseSpecError::UnknownFormat(spec.to_string()));
    }
    let name = &spec[..at_pos];
    let version = &spec[at_pos + 1..];
    if name.is_empty() || version.is_empty() {
        return Err(ParseSpecError::UnknownFormat(spec.to_string()));
    }
    Ok(InstallTarget::Npm {
        name: name.to_string(),
        version: Some(version.to_string()),
    })
}

/// Detect SCP-style SSH git URLs like `git@github.com:user/repo.git`.
///
/// These contain `@` with a `:` after it, no `://` (already handled above),
/// and the `@` is not at position 0 (which would be an npm scoped package).
fn contains_at_syntax(s: &str) -> bool {
    if let Some(at_pos) = s.find('@') {
        if at_pos == 0 {
            return false;
        }
        if let Some(col_pos) = s[at_pos..].find(':') {
            // Ensure there's something after the colon
            let after_colon = &s[at_pos + col_pos + 1..];
            return !after_colon.is_empty();
        }
    }
    false
}

/// Split "url#fragment" into (url, Some(fragment)) or (url, None).
fn split_fragment(s: &str) -> (&str, Option<String>) {
    if let Some(pos) = s.rfind('#') {
        let fragment = &s[pos + 1..];
        if fragment.is_empty() {
            (&s[..pos], None)
        } else {
            (&s[..pos], Some(fragment.to_string()))
        }
    } else {
        (s, None)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    // -----------------------------------------------------------------------
    // Npm plain name
    // -----------------------------------------------------------------------
    #[test]
    fn test_npm_plain_name() {
        let result = parse_install_spec("react").unwrap();
        assert_eq!(
            result,
            InstallTarget::Npm {
                name: "react".into(),
                version: None
            }
        );
    }

    #[test]
    fn test_npm_plain_name_with_dashes() {
        let result = parse_install_spec("typescript-eslint").unwrap();
        assert_eq!(
            result,
            InstallTarget::Npm {
                name: "typescript-eslint".into(),
                version: None
            }
        );
    }

    // -----------------------------------------------------------------------
    // Npm with version / range
    // -----------------------------------------------------------------------
    #[test]
    fn test_npm_with_exact_version() {
        let result = parse_install_spec("react@18.2.0").unwrap();
        assert_eq!(
            result,
            InstallTarget::Npm {
                name: "react".into(),
                version: Some("18.2.0".into())
            }
        );
    }

    #[test]
    fn test_npm_with_caret_range() {
        let result = parse_install_spec("react@^18").unwrap();
        assert_eq!(
            result,
            InstallTarget::Npm {
                name: "react".into(),
                version: Some("^18".into())
            }
        );
    }

    #[test]
    fn test_npm_with_tilde_range() {
        let result = parse_install_spec("react@~18.2").unwrap();
        assert_eq!(
            result,
            InstallTarget::Npm {
                name: "react".into(),
                version: Some("~18.2".into())
            }
        );
    }

    // -----------------------------------------------------------------------
    // Npm scoped packages
    // -----------------------------------------------------------------------
    #[test]
    fn test_npm_scoped_no_version() {
        let result = parse_install_spec("@angular/core").unwrap();
        assert_eq!(
            result,
            InstallTarget::Npm {
                name: "@angular/core".into(),
                version: None
            }
        );
    }

    #[test]
    fn test_npm_scoped_with_version() {
        let result = parse_install_spec("@angular/core@17.0.0").unwrap();
        assert_eq!(
            result,
            InstallTarget::Npm {
                name: "@angular/core".into(),
                version: Some("17.0.0".into())
            }
        );
    }

    #[test]
    fn test_npm_scoped_with_range() {
        let result = parse_install_spec("@angular/core@^17").unwrap();
        assert_eq!(
            result,
            InstallTarget::Npm {
                name: "@angular/core".into(),
                version: Some("^17".into())
            }
        );
    }

    #[test]
    fn test_npm_scoped_with_tilde() {
        let result = parse_install_spec("@angular/core@~17.0").unwrap();
        assert_eq!(
            result,
            InstallTarget::Npm {
                name: "@angular/core".into(),
                version: Some("~17.0".into())
            }
        );
    }

    // -----------------------------------------------------------------------
    // GitHub shorthand
    // -----------------------------------------------------------------------
    #[test]
    fn test_github_shorthand_basic() {
        let result = parse_install_spec("user/repo").unwrap();
        assert_eq!(
            result,
            InstallTarget::Github {
                repo: "user/repo".into(),
                commit: None
            }
        );
    }

    #[test]
    fn test_github_shorthand_with_commit() {
        let result = parse_install_spec("user/repo#abc1234").unwrap();
        assert_eq!(
            result,
            InstallTarget::Github {
                repo: "user/repo".into(),
                commit: Some("abc1234".into())
            }
        );
    }

    #[test]
    fn test_github_shorthand_with_branch() {
        let result = parse_install_spec("user/repo#develop").unwrap();
        assert_eq!(
            result,
            InstallTarget::Github {
                repo: "user/repo".into(),
                commit: Some("develop".into())
            }
        );
    }

    #[test]
    fn test_github_shorthand_with_tag() {
        let result = parse_install_spec("user/repo#v1.0.0").unwrap();
        assert_eq!(
            result,
            InstallTarget::Github {
                repo: "user/repo".into(),
                commit: Some("v1.0.0".into())
            }
        );
    }

    // -----------------------------------------------------------------------
    // Git URLs
    // -----------------------------------------------------------------------
    #[test]
    fn test_git_ssh_url() {
        let result = parse_install_spec("git@github.com:user/repo.git").unwrap();
        assert_eq!(
            result,
            InstallTarget::Git {
                url: "git@github.com:user/repo.git".into(),
                commit: None
            }
        );
    }

    #[test]
    fn test_git_domain_without_protocol() {
        let result = parse_install_spec("bitbucket.org/user/repo.git").unwrap();
        assert_eq!(
            result,
            InstallTarget::Git {
                url: "bitbucket.org/user/repo.git".into(),
                commit: None
            }
        );
    }

    #[test]
    fn test_git_domain_without_protocol_with_commit() {
        let result = parse_install_spec("bitbucket.org/user/repo.git#v1.0").unwrap();
        assert_eq!(
            result,
            InstallTarget::Git {
                url: "bitbucket.org/user/repo.git".into(),
                commit: Some("v1.0".into())
            }
        );
    }

    #[test]
    fn test_git_url_with_commit() {
        let result = parse_install_spec("https://github.com/user/repo.git#abc1234").unwrap();
        assert_eq!(
            result,
            InstallTarget::Git {
                url: "https://github.com/user/repo.git".into(),
                commit: Some("abc1234".into())
            }
        );
    }

    #[test]
    fn test_git_url_with_tag() {
        let result = parse_install_spec("https://github.com/user/repo.git#v1.0.0").unwrap();
        assert_eq!(
            result,
            InstallTarget::Git {
                url: "https://github.com/user/repo.git".into(),
                commit: Some("v1.0.0".into())
            }
        );
    }

    #[test]
    fn test_git_url_without_git_suffix() {
        let result = parse_install_spec("https://github.com/user/repo").unwrap();
        assert_eq!(
            result,
            InstallTarget::Git {
                url: "https://github.com/user/repo".into(),
                commit: None
            }
        );
    }

    #[test]
    fn test_git_ssh_url_with_commit() {
        let result = parse_install_spec("git@github.com:user/repo.git#abc123").unwrap();
        assert_eq!(
            result,
            InstallTarget::Git {
                url: "git@github.com:user/repo.git".into(),
                commit: Some("abc123".into())
            }
        );
    }

    // -----------------------------------------------------------------------
    // Tarball URLs
    // -----------------------------------------------------------------------
    #[test]
    fn test_tarball_url_tgz() {
        let result = parse_install_spec("https://example.com/pkg-1.2.3.tgz").unwrap();
        assert_eq!(
            result,
            InstallTarget::Tarball {
                url: "https://example.com/pkg-1.2.3.tgz".into()
            }
        );
    }

    #[test]
    fn test_tarball_url_tar_gz() {
        let result = parse_install_spec("https://example.com/pkg-1.2.3.tar.gz").unwrap();
        assert_eq!(
            result,
            InstallTarget::Tarball {
                url: "https://example.com/pkg-1.2.3.tar.gz".into()
            }
        );
    }

    #[test]
    fn test_tarball_local_path() {
        let result = parse_install_spec("./downloads/pkg.tgz").unwrap();
        assert_eq!(
            result,
            InstallTarget::Tarball {
                url: "./downloads/pkg.tgz".into()
            }
        );
    }

    #[test]
    fn test_tarball_local_path_tar_gz() {
        let result = parse_install_spec("/tmp/pkg.tar.gz").unwrap();
        assert_eq!(
            result,
            InstallTarget::Tarball {
                url: "/tmp/pkg.tar.gz".into()
            }
        );
    }

    // -----------------------------------------------------------------------
    // Display trait
    // -----------------------------------------------------------------------
    #[test]
    fn test_display_npm_no_version() {
        let t = InstallTarget::Npm {
            name: "react".into(),
            version: None,
        };
        assert_eq!(t.to_string(), "react");
    }

    #[test]
    fn test_display_npm_with_version() {
        let t = InstallTarget::Npm {
            name: "react".into(),
            version: Some("18.2.0".into()),
        };
        assert_eq!(t.to_string(), "react@18.2.0");
    }

    #[test]
    fn test_display_github() {
        let t = InstallTarget::Github {
            repo: "user/repo".into(),
            commit: Some("abc123".into()),
        };
        assert_eq!(t.to_string(), "user/repo#abc123");
    }

    #[test]
    fn test_display_git() {
        let t = InstallTarget::Git {
            url: "https://github.com/user/repo.git".into(),
            commit: None,
        };
        assert_eq!(t.to_string(), "https://github.com/user/repo.git");
    }

    #[test]
    fn test_display_tarball() {
        let t = InstallTarget::Tarball {
            url: "https://example.com/pkg.tgz".into(),
        };
        assert_eq!(t.to_string(), "https://example.com/pkg.tgz");
    }

    // -----------------------------------------------------------------------
    // Error cases
    // -----------------------------------------------------------------------
    #[test]
    fn test_empty_spec() {
        let err = parse_install_spec("").unwrap_err();
        assert!(matches!(err, ParseSpecError::Empty));
    }

    #[test]
    fn test_whitespace_only() {
        let err = parse_install_spec("   ").unwrap_err();
        assert!(matches!(err, ParseSpecError::Empty));
    }

    #[test]
    fn test_github_invalid_no_user() {
        let err = parse_install_spec("/repo").unwrap_err();
        assert!(matches!(err, ParseSpecError::UnknownFormat(_)));
    }

    #[test]
    fn test_github_invalid_no_repo() {
        let err = parse_install_spec("user/").unwrap_err();
        assert!(matches!(err, ParseSpecError::UnknownFormat(_)));
    }

    #[test]
    fn test_npm_version_only_at() {
        // "@" is treated as a valid npm package name (unusual but not Ara's job to validate)
        let result = parse_install_spec("@").unwrap();
        assert_eq!(
            result,
            InstallTarget::Npm {
                name: "@".into(),
                version: None
            }
        );
    }

    #[test]
    fn test_npm_empty_version() {
        let err = parse_install_spec("react@").unwrap_err();
        assert!(matches!(err, ParseSpecError::UnknownFormat(_)));
    }

    #[test]
    fn test_npm_version_like_name() {
        // "@18.2.0" is treated as a package name (npm naming rules not enforced here)
        let result = parse_install_spec("@18.2.0").unwrap();
        assert_eq!(
            result,
            InstallTarget::Npm {
                name: "@18.2.0".into(),
                version: None
            }
        );
    }
}
