use std::collections::HashMap;

use crate::types::DependencyEntry;
use ara_types::Constraint;

#[derive(Debug, thiserror::Error)]
pub enum CatalogResolveError {
    #[error("named catalog '{0}' does not exist")]
    CatalogNotFound(String),

    #[error("package '{package}' not found in catalog '{catalog}'")]
    PackageNotInCatalog { package: String, catalog: String },

    #[error("malformed catalog reference '{0}'")]
    MalformedRef(String),

    #[error("invalid constraint in catalog entry '{package}={version}': {detail}")]
    InvalidConstraint {
        package: String,
        version: String,
        detail: String,
    },
}

#[derive(Debug, Clone)]
pub enum CatalogWarning {
    Override {
        member: String,
        package: String,
        catalog_version: String,
        member_version: String,
    },
}

impl std::fmt::Display for CatalogWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Override {
                member,
                package,
                catalog_version,
                member_version,
            } => {
                write!(
                    f,
                    "package \"{member}\" overrides catalog entry {package} ({catalog_version}) with {member_version}"
                )
            }
        }
    }
}

/// Expand catalog references in dependency entries.
///
/// For each dependency with a `catalog:` prefix, looks up the actual constraint
/// from the workspace catalog. Dependencies without the prefix that match a
/// catalog entry emit a warning.
///
/// Returns warnings for overrides. Errors are returned for missing catalogs
/// or packages not found in the referenced catalog.
pub fn resolve_catalog_refs(
    deps: &mut [DependencyEntry],
    catalog: &HashMap<String, String>,
    catalogs: &HashMap<String, HashMap<String, String>>,
    member_name: &str,
) -> Result<Vec<CatalogWarning>, CatalogResolveError> {
    let mut warnings = Vec::new();

    for dep in deps.iter_mut() {
        if !dep.is_catalog_ref() {
            if let Some(cat_ver) = catalog.get(&dep.name) {
                warnings.push(CatalogWarning::Override {
                    member: member_name.to_string(),
                    package: dep.name.clone(),
                    catalog_version: cat_ver.clone(),
                    member_version: dep.version.clone().unwrap_or_else(|| "*".to_string()),
                });
            }
            continue;
        }

        let cr = dep.catalog_ref().ok_or_else(|| {
            CatalogResolveError::MalformedRef(dep.version.clone().unwrap_or_default())
        })?;

        let entry = if cr.catalog_name.is_empty() {
            catalog.get(&cr.package_name)
        } else {
            let named = catalogs
                .get(&cr.catalog_name)
                .ok_or_else(|| CatalogResolveError::CatalogNotFound(cr.catalog_name.clone()))?;
            named.get(&cr.package_name)
        };

        let constraint_str = entry.ok_or_else(|| CatalogResolveError::PackageNotInCatalog {
            package: cr.package_name.clone(),
            catalog: if cr.catalog_name.is_empty() {
                "default".to_string()
            } else {
                cr.catalog_name.clone()
            },
        })?;

        Constraint::parse(constraint_str).map_err(|e| CatalogResolveError::InvalidConstraint {
            package: cr.package_name.clone(),
            version: constraint_str.clone(),
            detail: e.to_string(),
        })?;

        dep.version = Some(constraint_str.clone());
    }

    Ok(warnings)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn make_catalog() -> (
        HashMap<String, String>,
        HashMap<String, HashMap<String, String>>,
    ) {
        let catalog = HashMap::from([
            ("react".to_string(), "^19.0.0".to_string()),
            ("react-dom".to_string(), "^19.0.0".to_string()),
        ]);
        let testing = HashMap::from([("jest".to_string(), "30.0.0".to_string())]);
        let catalogs = HashMap::from([("testing".to_string(), testing)]);
        (catalog, catalogs)
    }

    #[test]
    fn test_resolve_default_catalog_ref() {
        let (cat, cats) = make_catalog();
        let mut deps = vec![DependencyEntry {
            name: "react".to_string(),
            source: "npm".to_string(),
            kind: None,
            version: Some("catalog:".to_string()),
            repo: None,
            url: None,
            commit: None,
            path: None,
        }];

        let warnings = resolve_catalog_refs(&mut deps, &cat, &cats, "pkg-a").unwrap();
        assert!(warnings.is_empty());
        assert_eq!(deps[0].version.as_deref(), Some("^19.0.0"));
    }

    #[test]
    fn test_resolve_named_catalog_ref() {
        let (cat, cats) = make_catalog();
        let mut deps = vec![DependencyEntry {
            name: "jest".to_string(),
            source: "npm".to_string(),
            kind: None,
            version: Some("catalog:testing".to_string()),
            repo: None,
            url: None,
            commit: None,
            path: None,
        }];

        let warnings = resolve_catalog_refs(&mut deps, &cat, &cats, "pkg-b").unwrap();
        assert!(warnings.is_empty());
        assert_eq!(deps[0].version.as_deref(), Some("30.0.0"));
    }

    #[test]
    fn test_catalog_not_found_error() {
        let (cat, cats) = make_catalog();
        let mut deps = vec![DependencyEntry {
            name: "jest".to_string(),
            source: "npm".to_string(),
            kind: None,
            version: Some("catalog:missing".to_string()),
            repo: None,
            url: None,
            commit: None,
            path: None,
        }];

        let err = resolve_catalog_refs(&mut deps, &cat, &cats, "pkg-c").unwrap_err();
        assert!(matches!(err, CatalogResolveError::CatalogNotFound(_)));
    }

    #[test]
    fn test_package_not_in_catalog_error() {
        let (cat, cats) = make_catalog();
        let mut deps = vec![DependencyEntry {
            name: "nonexistent".to_string(),
            source: "npm".to_string(),
            kind: None,
            version: Some("catalog:".to_string()),
            repo: None,
            url: None,
            commit: None,
            path: None,
        }];

        let err = resolve_catalog_refs(&mut deps, &cat, &cats, "pkg-d").unwrap_err();
        assert!(
            matches!(err, CatalogResolveError::PackageNotInCatalog { .. }),
            "expected PackageNotInCatalog, got {err:?}"
        );
    }

    #[test]
    fn test_override_warning() {
        let (cat, cats) = make_catalog();
        let mut deps = vec![DependencyEntry {
            name: "react".to_string(),
            source: "npm".to_string(),
            kind: None,
            version: Some(">=18 <19".to_string()),
            repo: None,
            url: None,
            commit: None,
            path: None,
        }];

        let warnings = resolve_catalog_refs(&mut deps, &cat, &cats, "legacy-app").unwrap();
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            CatalogWarning::Override {
                member,
                package,
                catalog_version: _,
                member_version: _,
            } => {
                assert_eq!(member, "legacy-app");
                assert_eq!(package, "react");
            }
        }
        // Version should NOT be changed for overrides
        assert_eq!(deps[0].version.as_deref(), Some(">=18 <19"));
    }

    #[test]
    fn test_regular_dep_no_catalog_match_no_warning() {
        let (cat, cats) = make_catalog();
        let mut deps = vec![DependencyEntry {
            name: "some-other-pkg".to_string(),
            source: "npm".to_string(),
            kind: None,
            version: Some("^1.0.0".to_string()),
            repo: None,
            url: None,
            commit: None,
            path: None,
        }];

        let warnings = resolve_catalog_refs(&mut deps, &cat, &cats, "pkg-e").unwrap();
        assert!(warnings.is_empty());
        assert_eq!(deps[0].version.as_deref(), Some("^1.0.0"));
    }

    #[test]
    fn test_invalid_constraint_in_catalog() {
        let mut cat = HashMap::new();
        cat.insert("bad".to_string(), "not-a-valid-constraint!!".to_string());
        let cats = HashMap::new();

        let mut deps = vec![DependencyEntry {
            name: "bad".to_string(),
            source: "npm".to_string(),
            kind: None,
            version: Some("catalog:".to_string()),
            repo: None,
            url: None,
            commit: None,
            path: None,
        }];

        let err = resolve_catalog_refs(&mut deps, &cat, &cats, "pkg-f").unwrap_err();
        assert!(
            matches!(err, CatalogResolveError::InvalidConstraint { .. }),
            "expected InvalidConstraint, got {err:?}"
        );
    }

    #[test]
    fn test_is_catalog_ref() {
        let yes = DependencyEntry {
            name: "react".to_string(),
            source: "npm".to_string(),
            kind: None,
            version: Some("catalog:".to_string()),
            repo: None,
            url: None,
            commit: None,
            path: None,
        };
        assert!(yes.is_catalog_ref());

        let named = DependencyEntry {
            name: "jest".to_string(),
            source: "npm".to_string(),
            kind: None,
            version: Some("catalog:testing".to_string()),
            repo: None,
            url: None,
            commit: None,
            path: None,
        };
        assert!(named.is_catalog_ref());

        let no = DependencyEntry {
            name: "zod".to_string(),
            source: "npm".to_string(),
            kind: None,
            version: Some("^3.0.0".to_string()),
            repo: None,
            url: None,
            commit: None,
            path: None,
        };
        assert!(!no.is_catalog_ref());
    }

    #[test]
    fn test_catalog_ref_parse_default() {
        let dep = DependencyEntry {
            name: "react".to_string(),
            source: "npm".to_string(),
            kind: None,
            version: Some("catalog:".to_string()),
            repo: None,
            url: None,
            commit: None,
            path: None,
        };
        let cr = dep.catalog_ref().unwrap();
        assert!(cr.catalog_name.is_empty());
        assert_eq!(cr.package_name, "react");
    }

    #[test]
    fn test_catalog_ref_parse_named() {
        let dep = DependencyEntry {
            name: "jest".to_string(),
            source: "npm".to_string(),
            kind: None,
            version: Some("catalog:testing".to_string()),
            repo: None,
            url: None,
            commit: None,
            path: None,
        };
        let cr = dep.catalog_ref().unwrap();
        assert_eq!(cr.catalog_name, "testing");
        assert_eq!(cr.package_name, "jest");
    }

    #[test]
    fn test_catalog_ref_parse_regular_version() {
        let dep = DependencyEntry {
            name: "zod".to_string(),
            source: "npm".to_string(),
            kind: None,
            version: Some("^3.0.0".to_string()),
            repo: None,
            url: None,
            commit: None,
            path: None,
        };
        assert!(dep.catalog_ref().is_none());
    }

    #[test]
    fn test_catalog_ref_parse_no_version() {
        let dep = DependencyEntry {
            name: "zod".to_string(),
            source: "npm".to_string(),
            kind: None,
            version: None,
            repo: None,
            url: None,
            commit: None,
            path: None,
        };
        assert!(dep.catalog_ref().is_none());
    }
}
