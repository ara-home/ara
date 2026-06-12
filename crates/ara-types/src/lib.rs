//! Core types for the ara package manager: versions, constraints, source types,
//! risk levels, and security analysis results.

use serde::Serialize;
use std::fmt;
use std::str::FromStr;

// ---------------------------------------------------------------------------
// SourceType
// ---------------------------------------------------------------------------

/// The origin of a package dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SourceType {
    Workspace,
    Local,
    Git,
    Github,
    Registry,
    Npm,
    Url,
}

impl fmt::Display for SourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Workspace => write!(f, "workspace"),
            Self::Local => write!(f, "local"),
            Self::Git => write!(f, "git"),
            Self::Github => write!(f, "github"),
            Self::Registry => write!(f, "registry"),
            Self::Npm => write!(f, "npm"),
            Self::Url => write!(f, "url"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unknown source type: '{0}'")]
pub struct UnknownSourceType(pub String);

impl FromStr for SourceType {
    type Err = UnknownSourceType;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "workspace" => Ok(Self::Workspace),
            "local" => Ok(Self::Local),
            "git" => Ok(Self::Git),
            "github" => Ok(Self::Github),
            "registry" => Ok(Self::Registry),
            "npm" => Ok(Self::Npm),
            "url" => Ok(Self::Url),
            _ => Err(UnknownSourceType(s.to_owned())),
        }
    }
}

// ---------------------------------------------------------------------------
// Version (re-exported from the semver crate)
// ---------------------------------------------------------------------------

/// A semantic version (major.minor.patch) with optional prerelease and build metadata.
pub use semver::Version;

// ---------------------------------------------------------------------------
// WildcardParts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WildcardParts {
    pub major: u64,
    pub minor: Option<u64>,
}

// ---------------------------------------------------------------------------
// Constraint
// ---------------------------------------------------------------------------

/// A version constraint (^, ~, >=, <=, >, <, exact, wildcard, or AND-combined).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constraint {
    Exact(Version),
    Caret(Version),
    Tilde(Version),
    GreaterOrEqual(Version),
    GreaterThan(Version),
    LessOrEqual(Version),
    LessThan(Version),
    Wildcard(WildcardParts),
    /// AND-combined constraints (space-separated in npm notation, e.g. `>=1.0.0 <2.0.0`).
    And(Vec<Constraint>),
}

#[derive(Debug, thiserror::Error)]
pub enum ConstraintParseError {
    #[error("empty constraint string")]
    Empty,
    #[error("invalid version: {0}")]
    InvalidVersion(String),
}

fn split_wildcard(s: &str) -> WildcardParts {
    let dot = s.find('.');
    let major_str = dot.map_or(s, |d| &s[..d]);
    let major = major_str.parse::<u64>().unwrap_or(0);

    let minor = dot.and_then(|d| {
        let rest = &s[d + 1..];
        let second_dot = rest.find('.');
        second_dot.map_or_else(
            || {
                if rest.is_empty() || rest == "x" {
                    None
                } else {
                    rest.parse::<u64>().ok()
                }
            },
            |sd| rest[..sd].parse::<u64>().ok(),
        )
    });

    WildcardParts { major, minor }
}

/// Remove whitespace between operator prefix (^, ~, >=, <=, >, <) and version number.
/// npm allows spaces between operators and versions, e.g. `>= 2.1.2`.
fn normalize_operators(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < s.len() {
        if bytes[i] == b'^' || bytes[i] == b'~' {
            out.push(bytes[i] as char);
            i += 1;
            while i < s.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
        } else if i + 1 < s.len()
            && ((bytes[i] == b'>' && bytes[i + 1] == b'=')
                || (bytes[i] == b'<' && bytes[i + 1] == b'='))
        {
            out.push(bytes[i] as char);
            out.push(bytes[i + 1] as char);
            i += 2;
            while i < s.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
        } else if bytes[i] == b'>' || bytes[i] == b'<' {
            out.push(bytes[i] as char);
            i += 1;
            while i < s.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

impl Constraint {
    /// Parse a version string, expanding shorthand forms like "1" → "1.0.0", "1.2" → "1.2.0".
    fn parse_version(s: &str) -> Result<Version, ConstraintParseError> {
        let expanded = match s.chars().filter(|&c| c == '.').count() {
            0 => format!("{s}.0.0"),
            1 => format!("{s}.0"),
            _ => s.to_string(),
        };
        semver::Version::parse(&expanded)
            .map_err(|e| ConstraintParseError::InvalidVersion(e.to_string()))
    }

    pub fn parse(s: &str) -> Result<Self, ConstraintParseError> {
        if s.is_empty() {
            return Err(ConstraintParseError::Empty);
        }

        // Normalize: remove whitespace between operator and version (npm allows ">= 2.1.2")
        let s = normalize_operators(s);

        // Compound constraint: space-separated AND (npm notation, e.g. ">=2.1.2 <3.0.0")
        if s.contains(' ') {
            let parts: Vec<&str> = s.split_whitespace().collect();
            if parts.len() > 1 {
                let constraints: Result<Vec<Constraint>, _> =
                    parts.iter().map(|p| Self::parse(p)).collect();
                return Ok(Self::And(constraints?));
            }
        }

        let bytes = s.as_bytes();
        if bytes[0] == b'^' {
            return Ok(Self::Caret(Self::parse_version(&s[1..])?));
        }
        if bytes[0] == b'~' {
            return Ok(Self::Tilde(Self::parse_version(&s[1..])?));
        }
        if bytes[0] == b'>' {
            if s.len() > 1 && bytes[1] == b'=' {
                return Ok(Self::GreaterOrEqual(Self::parse_version(&s[2..])?));
            }
            return Ok(Self::GreaterThan(Self::parse_version(&s[1..])?));
        }
        if bytes[0] == b'<' {
            if s.len() > 1 && bytes[1] == b'=' {
                return Ok(Self::LessOrEqual(Self::parse_version(&s[2..])?));
            }
            return Ok(Self::LessThan(Self::parse_version(&s[1..])?));
        }

        if s == "*" {
            return Ok(Self::Wildcard(WildcardParts {
                major: u64::MAX,
                minor: None,
            }));
        }

        if s.contains('x') {
            return Ok(Self::Wildcard(split_wildcard(&s)));
        }

        Ok(Self::Exact(Self::parse_version(&s)?))
    }

    /// Per npm semver convention, a prerelease version should only satisfy a
    /// constraint when the constraint's base version also carries a prerelease
    /// identifier. If the constraint version is a release and the candidate is
    /// a prerelease, they should not match.
    fn prerelease_mismatch(constraint_ver: &Version, candidate: &Version) -> bool {
        constraint_ver.pre.is_empty() && !candidate.pre.is_empty()
    }

    #[must_use]
    pub fn satisfied_by(&self, version: &Version) -> bool {
        match self {
            Self::Exact(v) => version == v,
            Self::Caret(v) => {
                if Self::prerelease_mismatch(v, version) {
                    return false;
                }
                if version.major != v.major {
                    return false;
                }
                if v.major == 0 {
                    if v.minor != version.minor {
                        return false;
                    }
                    return version.patch >= v.patch;
                }
                version >= v
            }
            Self::Tilde(v) => {
                if Self::prerelease_mismatch(v, version) {
                    return false;
                }
                if version.major != v.major {
                    return false;
                }
                if version.minor != v.minor {
                    return false;
                }
                version.patch >= v.patch
            }
            Self::GreaterOrEqual(v) => {
                if Self::prerelease_mismatch(v, version) {
                    return false;
                }
                version >= v
            }
            Self::GreaterThan(v) => {
                if Self::prerelease_mismatch(v, version) {
                    return false;
                }
                version > v
            }
            Self::LessOrEqual(v) => {
                if Self::prerelease_mismatch(v, version) {
                    return false;
                }
                version <= v
            }
            Self::LessThan(v) => {
                if Self::prerelease_mismatch(v, version) {
                    return false;
                }
                version < v
            }
            Self::Wildcard(w) => {
                if w.major == u64::MAX {
                    return true;
                }
                if version.major != w.major {
                    return false;
                }
                if let Some(m) = w.minor {
                    if version.minor != m {
                        return false;
                    }
                }
                true
            }
            Self::And(constraints) => constraints.iter().all(|c| c.satisfied_by(version)),
        }
    }
}

impl fmt::Display for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exact(v) => write!(f, "{v}"),
            Self::Caret(v) => write!(f, "^{v}"),
            Self::Tilde(v) => write!(f, "~{v}"),
            Self::GreaterOrEqual(v) => write!(f, ">={v}"),
            Self::GreaterThan(v) => write!(f, ">{v}"),
            Self::LessOrEqual(v) => write!(f, "<={v}"),
            Self::LessThan(v) => write!(f, "<{v}"),
            Self::Wildcard(w) => {
                if w.major == u64::MAX {
                    write!(f, "*")
                } else if let Some(m) = w.minor {
                    write!(f, "{}.{}.x", w.major, m)
                } else {
                    write!(f, "{}.x", w.major)
                }
            }
            Self::And(constraints) => {
                for (i, c) in constraints.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{c}")?;
                }
                Ok(())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PackageIdentity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageIdentity {
    pub source: SourceType,
    pub name: String,
    pub version: Version,
    pub content_hash: Option<String>,
    /// Optional ref (branch, tag, commit SHA) for git/GitHub sources.
    pub requested_ref: Option<String>,
}

// ---------------------------------------------------------------------------
// RiskLevel
// ---------------------------------------------------------------------------

/// Severity level for a security finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

// ---------------------------------------------------------------------------
// Security analysis types
// ---------------------------------------------------------------------------

/// A single security finding discovered during package analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub pattern: String,
    pub severity: RiskLevel,
    pub location: Option<String>,
    pub description: String,
}

/// The result of analyzing a package for security patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisResult {
    pub risk_level: RiskLevel,
    pub findings: Vec<Finding>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    // ---- Version ----

    #[test]
    fn test_version_parse_basic() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
        assert!(v.pre.is_empty());
        assert!(v.build.is_empty());
    }

    #[test]
    fn test_version_parse_prerelease() {
        let v = Version::parse("1.2.3-alpha.1+build.42").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
        assert_eq!(v.pre.to_string(), "alpha.1");
        assert_eq!(v.build.to_string(), "build.42");
    }

    #[test]
    fn test_version_ordering() {
        let v1 = Version::parse("1.2.3").unwrap();
        let v2 = Version::parse("2.0.0").unwrap();
        assert!(v1 < v2);
        assert!(v2 > v1);
        assert_eq!(v1, v1);
    }

    #[test]
    fn test_version_ordering_with_prerelease() {
        let release = Version::parse("1.0.0").unwrap();
        let prerelease = Version::parse("1.0.0-rc.1").unwrap();
        assert!(release > prerelease);
        assert!(prerelease < release);
        let same = Version::parse("1.0.0-rc.1").unwrap();
        assert_eq!(prerelease, same);
    }

    #[test]
    fn test_version_prerelease_edge_cases() {
        let v = Version::parse("1.0.0-0").unwrap();
        assert_eq!(v.pre.to_string(), "0");

        let v2 = Version::parse("1.0.0+build").unwrap();
        assert!(v2.pre.is_empty());
        assert_eq!(v2.build.to_string(), "build");

        let v3 = Version::parse("1.0.0-rc.1+build.42").unwrap();
        assert_eq!(v3.pre.to_string(), "rc.1");
        assert_eq!(v3.build.to_string(), "build.42");
    }

    #[test]
    fn test_version_invalid_inputs() {
        assert!(Version::parse("").is_err());
        assert!(Version::parse("1").is_err());
        assert!(Version::parse("1.").is_err());
        assert!(Version::parse("1.2").is_err());
        assert!(Version::parse("1.2.").is_err());
        assert!(Version::parse(".1.2.3").is_err());
        assert!(Version::parse("a.b.c").is_err());
        assert!(Version::parse("1.2.3.").is_err());
    }

    #[test]
    fn test_version_overflow() {
        assert!(Version::parse("99999999999999999999.0.0").is_err());
    }

    #[test]
    fn test_version_display() {
        let v = Version::parse("1.2.3-alpha.1+build.42").unwrap();
        assert_eq!(v.to_string(), "1.2.3-alpha.1+build.42");

        let v2 = Version::parse("1.0.0").unwrap();
        assert_eq!(v2.to_string(), "1.0.0");
    }

    // ---- Constraint ----

    #[test]
    fn test_constraint_exact() {
        let c = Constraint::parse("1.2.3").unwrap();
        assert!(c.satisfied_by(&Version::parse("1.2.3").unwrap()));
        assert!(!c.satisfied_by(&Version::parse("1.2.4").unwrap()));
    }

    #[test]
    fn test_constraint_caret() {
        let c = Constraint::parse("^1.2.3").unwrap();
        assert!(c.satisfied_by(&Version::parse("1.5.0").unwrap()));
        assert!(!c.satisfied_by(&Version::parse("2.0.0").unwrap()));
    }

    #[test]
    fn test_constraint_caret_major_zero() {
        let c = Constraint::parse("^0.1.2").unwrap();
        assert!(c.satisfied_by(&Version::parse("0.1.2").unwrap()));
        assert!(c.satisfied_by(&Version::parse("0.1.9").unwrap()));
        assert!(!c.satisfied_by(&Version::parse("0.2.0").unwrap()));
        assert!(!c.satisfied_by(&Version::parse("1.0.0").unwrap()));
    }

    #[test]
    fn test_constraint_tilde() {
        let c = Constraint::parse("~1.2.3").unwrap();
        assert!(c.satisfied_by(&Version::parse("1.2.3").unwrap()));
        assert!(c.satisfied_by(&Version::parse("1.2.9").unwrap()));
        assert!(!c.satisfied_by(&Version::parse("1.3.0").unwrap()));
        assert!(!c.satisfied_by(&Version::parse("2.0.0").unwrap()));
    }

    #[test]
    fn test_constraint_greater_or_equal() {
        let c = Constraint::parse(">=2.0.0").unwrap();
        assert!(c.satisfied_by(&Version::parse("2.0.0").unwrap()));
        assert!(c.satisfied_by(&Version::parse("3.0.0").unwrap()));
        assert!(!c.satisfied_by(&Version::parse("1.9.9").unwrap()));
    }

    #[test]
    fn test_constraint_greater_than() {
        let c = Constraint::parse(">1.0.0").unwrap();
        assert!(!c.satisfied_by(&Version::parse("1.0.0").unwrap()));
        assert!(c.satisfied_by(&Version::parse("1.0.1").unwrap()));
    }

    #[test]
    fn test_constraint_less_than() {
        let c = Constraint::parse("<2.0.0").unwrap();
        assert!(c.satisfied_by(&Version::parse("1.9.9").unwrap()));
        assert!(!c.satisfied_by(&Version::parse("2.0.0").unwrap()));
        assert!(!c.satisfied_by(&Version::parse("2.0.1").unwrap()));
    }

    #[test]
    fn test_constraint_less_or_equal() {
        let c = Constraint::parse("<=2.0.0").unwrap();
        assert!(c.satisfied_by(&Version::parse("2.0.0").unwrap()));
        assert!(c.satisfied_by(&Version::parse("1.0.0").unwrap()));
        assert!(!c.satisfied_by(&Version::parse("2.0.1").unwrap()));
    }

    #[test]
    fn test_constraint_wildcard_star() {
        let c = Constraint::parse("*").unwrap();
        assert!(c.satisfied_by(&Version::parse("0.0.0").unwrap()));
        assert!(c.satisfied_by(&Version::parse("99.99.99").unwrap()));
    }

    #[test]
    fn test_constraint_wildcard_minor() {
        let c = Constraint::parse("1.2.x").unwrap();
        assert!(c.satisfied_by(&Version::parse("1.2.0").unwrap()));
        assert!(c.satisfied_by(&Version::parse("1.2.99").unwrap()));
        assert!(!c.satisfied_by(&Version::parse("1.3.0").unwrap()));
    }

    #[test]
    fn test_constraint_wildcard_major() {
        let c = Constraint::parse("1.x").unwrap();
        assert!(c.satisfied_by(&Version::parse("1.2.3").unwrap()));
        assert!(!c.satisfied_by(&Version::parse("2.0.0").unwrap()));
    }

    #[test]
    fn test_constraint_invalid_inputs() {
        assert!(Constraint::parse("").is_err());
        assert!(Constraint::parse("^").is_err());
        assert!(Constraint::parse(">").is_err());
        assert!(Constraint::parse("<").is_err());
        assert!(Constraint::parse("~").is_err());
        assert!(Constraint::parse(">=").is_err());
        assert!(Constraint::parse("1.2.").is_err());
    }

    #[test]
    fn test_constraint_display() {
        let c = Constraint::parse("^1.2.3").unwrap();
        assert_eq!(c.to_string(), "^1.2.3");

        let c2 = Constraint::parse("*").unwrap();
        assert_eq!(c2.to_string(), "*");
    }

    // ---- SourceType ----

    #[test]
    fn test_source_type_parse_and_format() {
        assert_eq!(
            "workspace".parse::<SourceType>().unwrap(),
            SourceType::Workspace
        );
        assert_eq!(SourceType::Registry.to_string(), "registry");
        assert!("unknown".parse::<SourceType>().is_err());
    }

    #[test]
    fn test_source_type_all_roundtrip() {
        let variants = [
            SourceType::Workspace,
            SourceType::Local,
            SourceType::Git,
            SourceType::Github,
            SourceType::Registry,
            SourceType::Npm,
        ];
        for variant in &variants {
            let s = variant.to_string();
            let parsed: SourceType = s.parse().unwrap();
            assert_eq!(*variant, parsed);
        }
    }

    // ---- RiskLevel ----

    #[test]
    fn test_risk_level_order() {
        assert!(RiskLevel::Low < RiskLevel::Medium);
        assert!(RiskLevel::Medium < RiskLevel::High);
        assert!(RiskLevel::High < RiskLevel::Critical);
    }

    #[test]
    fn test_risk_level_display() {
        assert_eq!(RiskLevel::Low.to_string(), "low");
        assert_eq!(RiskLevel::Critical.to_string(), "critical");
    }

    // ---- Generative ----

    #[test]
    fn test_version_generative_roundtrip() {
        use std::num::Wrapping;

        let mut rng = Wrapping(42u64);
        for _ in 0..100 {
            let major = rng.0 % 101;
            rng.0 = rng.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            let minor = rng.0 % 101;
            rng.0 = rng.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            let patch = rng.0 % 101;
            rng.0 = rng.0.wrapping_mul(6364136223846793005).wrapping_add(1);

            let s = format!("{major}.{minor}.{patch}");
            let parsed = Version::parse(&s).unwrap();
            assert_eq!(parsed.major, major);
            assert_eq!(parsed.minor, minor);
            assert_eq!(parsed.patch, patch);
        }
    }

    #[test]
    fn test_constraint_generative_parse_does_not_crash() {
        use std::num::Wrapping;

        let mut rng = Wrapping(1234u64);
        for _ in 0..500 {
            let len = (rng.0 % 21) as usize;
            rng.0 = rng.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            let mut buf = [0u8; 20];
            for item in buf.iter_mut().take(len) {
                *item = 32 + (rng.0 % 95) as u8;
                rng.0 = rng.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            }
            let input = std::str::from_utf8(&buf[..len]).unwrap_or("");
            let _ = Constraint::parse(input);
        }
    }
}
