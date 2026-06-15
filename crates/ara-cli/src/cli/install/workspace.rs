use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::Result;

use ara_manifest::package_json;
use ara_manifest::parser;

fn read_member_manifest(member_dir: &Path) -> Option<ara_manifest::types::Manifest> {
    let member_toml = member_dir.join("ara.toml");
    if member_toml.exists() {
        let content = std::fs::read_to_string(&member_toml).ok()?;
        return parser::parse(&content).ok();
    }

    let member_pkg_json = member_dir.join("package.json");
    if member_pkg_json.exists() {
        let content = std::fs::read_to_string(&member_pkg_json).ok()?;
        return package_json::parse_package_json(&content).ok();
    }

    None
}

pub(crate) fn expand_workspace_members(
    workspace: &ara_manifest::types::Workspace,
    cwd: &Path,
) -> Vec<ara_manifest::types::DependencyEntry> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();

    let canonical_cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());

    for pattern in &workspace.members {
        let full_pattern = cwd.join(pattern);
        let full_str = full_pattern.to_string_lossy().to_string();

        let matches = match glob::glob(&full_str) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("  warning: invalid workspace pattern \"{pattern}\": {e}");
                continue;
            }
        };

        for entry in matches {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("  warning: glob error for pattern \"{pattern}\": {e}");
                    continue;
                }
            };

            if !entry.is_dir() {
                continue;
            }

            // Reject path traversal: verify resolved entry is within workspace root
            if let Ok(canonical_entry) = entry.canonicalize() {
                if !canonical_entry.starts_with(&canonical_cwd) {
                    eprintln!(
                        "  warning: workspace member {} escapes workspace root, skipping",
                        entry.display()
                    );
                    continue;
                }
            }

            let manifest = match read_member_manifest(&entry) {
                Some(m) => m,
                None => {
                    eprintln!(
                        "  warning: workspace member {} has no manifest, skipping",
                        entry.display()
                    );
                    continue;
                }
            };

            if !seen.insert(manifest.project.name.clone()) {
                continue;
            }

            let rel_path = entry
                .strip_prefix(cwd)
                .unwrap_or(&entry)
                .to_string_lossy()
                .to_string();

            entries.push(ara_manifest::types::DependencyEntry {
                name: manifest.project.name,
                source: "workspace".to_string(),
                kind: None,
                version: Some(manifest.project.version),
                path: Some(rel_path),
                repo: None,
                url: None,
                commit: None,
            });
        }
    }

    entries
}

pub(crate) fn read_manifest(cwd: &Path) -> Result<ara_manifest::types::Manifest> {
    use anyhow::Context;

    let manifest_path = cwd.join("ara.toml");
    let pkg_json_path = cwd.join("package.json");

    let mut final_manifest: Option<ara_manifest::types::Manifest> = None;

    if pkg_json_path.exists() {
        let content = std::fs::read_to_string(&pkg_json_path)
            .with_context(|| format!("failed to read {}", pkg_json_path.display()))?;
        let m =
            package_json::parse_package_json(&content).context("failed to parse package.json")?;
        final_manifest = Some(m);
    }

    if manifest_path.exists() {
        let content = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let m = parser::parse(&content).context("failed to parse ara.toml")?;

        if let Some(mut fm) = final_manifest {
            // merge ara.toml advanced settings into package.json manifest
            fm.security = m.security;
            fm.build = m.build;
            // Merge catalog from ara.toml workspace if present
            if let Some(toml_ws) = m.workspace {
                if toml_ws.catalog.is_some() || toml_ws.catalogs.is_some() {
                    match &mut fm.workspace {
                        Some(ref mut fm_ws) => {
                            if toml_ws.catalog.is_some() {
                                fm_ws.catalog = toml_ws.catalog;
                            }
                            if toml_ws.catalogs.is_some() {
                                fm_ws.catalogs = toml_ws.catalogs;
                            }
                        }
                        None => {
                            fm.workspace = Some(toml_ws);
                        }
                    }
                }
            }
            // Note: We deliberately do NOT merge deps, scripts, or members from ara.toml
            // because package.json is now the source of truth for them.
            final_manifest = Some(fm);
        } else {
            // if no package.json exists, fallback to ara.toml completely
            final_manifest = Some(m);
        }
    }

    if let Some(m) = final_manifest {
        return Ok(m);
    }

    Err(anyhow::anyhow!(
        "no manifest found: neither package.json nor ara.toml exists in {}",
        cwd.display()
    ))
}

/// Collect dependencies from workspace members and resolve catalog references.
///
/// Reads each member's manifest, resolves `catalog:` references against the
/// root workspace catalog, and emits override warnings.
pub(crate) fn collect_member_deps_with_catalog(
    workspace: &ara_manifest::types::Workspace,
    cwd: &Path,
    root_catalog: &HashMap<String, String>,
    root_catalogs: &HashMap<String, HashMap<String, String>>,
) -> Vec<ara_manifest::types::DependencyEntry> {
    let mut all_deps = Vec::new();
    let mut seen = HashSet::new();

    for pattern in &workspace.members {
        let full_pattern = cwd.join(pattern);
        let full_str = full_pattern.to_string_lossy().to_string();

        let matches = match glob::glob(&full_str) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("  warning: invalid workspace pattern \"{pattern}\": {e}");
                continue;
            }
        };

        for entry in matches {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("  warning: glob error for pattern \"{pattern}\": {e}");
                    continue;
                }
            };

            let member_path = entry;

            let canonical_cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
            if let Ok(canonical_entry) = member_path.canonicalize() {
                if !canonical_entry.starts_with(&canonical_cwd) {
                    continue;
                }
            }

            let manifest = match read_member_manifest(&member_path) {
                Some(m) => m,
                None => continue,
            };

            if !seen.insert(manifest.project.name.clone()) {
                continue;
            }

            if manifest.deps.is_empty() {
                continue;
            }

            let member_name = manifest.project.name.clone();
            let mut member_deps = manifest.deps;

            if let Err(e) = ara_manifest::catalog::resolve_catalog_refs(
                &mut member_deps,
                root_catalog,
                root_catalogs,
                &member_name,
            ) {
                eprintln!("  warning: catalog resolution for \"{member_name}\" failed: {e}");
                continue;
            }

            for dep in member_deps {
                if !seen.contains(&dep.name) {
                    all_deps.push(dep);
                }
            }
        }
    }

    all_deps
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn test_expand_workspace_members_500() {
        let root = tempfile::tempdir().unwrap();
        let packages = root.path().join("packages");
        std::fs::create_dir_all(&packages).unwrap();

        // Create 500 workspace members
        let mut member_names = Vec::new();
        for i in 0..500 {
            let name = format!("pkg-{i:04}");
            let dir = packages.join(&name);
            std::fs::create_dir_all(&dir).unwrap();
            let manifest = format!(
                r#"[project]
name = "{name}"
version = "0.1.0"
"#
            );
            std::fs::write(dir.join("ara.toml"), manifest).unwrap();
            member_names.push(name);
        }

        let ws = ara_manifest::types::Workspace {
            members: vec!["packages/*".to_string()],
            catalog: None,
            catalogs: None,
        };
        let entries = expand_workspace_members(&ws, root.path());
        assert_eq!(entries.len(), 500);

        let names: HashSet<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        for name in &member_names {
            assert!(names.contains(name.as_str()), "missing {name}");
        }
    }

    #[test]
    fn test_read_manifest_with_package_json() {
        let root = tempfile::tempdir().unwrap();
        let pkg_json =
            r#"{"name": "my-app", "version": "0.1.0", "dependencies": {"zod": "^3.0.0"}}"#;
        std::fs::write(root.path().join("package.json"), pkg_json).unwrap();

        let m = read_manifest(root.path()).unwrap();
        assert_eq!(m.project.name, "my-app");
        assert_eq!(m.deps.len(), 1);
        assert_eq!(m.deps[0].name, "zod");
    }

    #[test]
    fn test_read_manifest_merge_ara_toml() {
        let root = tempfile::tempdir().unwrap();
        // Both exist
        let pkg_json = r#"{"name": "from-pkg-json", "version": "1.0.0"}"#;
        std::fs::write(root.path().join("package.json"), pkg_json).unwrap();
        let ara_toml = r#"[project]
name = "ignored"
version = "ignored"

[security]
require_review = true
"#;
        std::fs::write(root.path().join("ara.toml"), ara_toml).unwrap();

        let m = read_manifest(root.path()).unwrap();
        // Should take name from package.json
        assert_eq!(m.project.name, "from-pkg-json");
        assert_eq!(m.project.version, "1.0.0");
        // Should take security from ara.toml
        assert_eq!(m.security.unwrap().require_review, Some(true));
    }

    #[test]
    fn test_read_manifest_neither() {
        let root = tempfile::tempdir().unwrap();
        assert!(read_manifest(root.path()).is_err());
    }

    #[test]
    fn test_expand_workspace_members_with_package_json() {
        let root = tempfile::tempdir().unwrap();
        let packages = root.path().join("packages");
        let member_dir = packages.join("pkg-a");
        std::fs::create_dir_all(&member_dir).unwrap();

        // Member with package.json instead of ara.toml
        let member_json = r#"{"name": "pkg-a", "version": "0.1.0"}"#;
        std::fs::write(member_dir.join("package.json"), member_json).unwrap();

        let ws = ara_manifest::types::Workspace {
            members: vec!["packages/*".to_string()],
            catalog: None,
            catalogs: None,
        };
        let entries = expand_workspace_members(&ws, root.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "pkg-a");
        assert_eq!(entries[0].version.as_deref(), Some("0.1.0"));
    }

    #[test]
    fn test_collect_member_deps_with_catalog_expands_refs() {
        let root = tempfile::tempdir().unwrap();
        let packages = root.path().join("packages");
        let member_dir = packages.join("pkg-a");
        std::fs::create_dir_all(&member_dir).unwrap();

        let member_json = r#"{
            "name": "pkg-a",
            "version": "0.1.0",
            "dependencies": {
                "react": "catalog:"
            }
        }"#;
        std::fs::write(member_dir.join("package.json"), member_json).unwrap();

        let ws = ara_manifest::types::Workspace {
            members: vec!["packages/*".to_string()],
            catalog: Some(HashMap::from([(
                "react".to_string(),
                "^19.0.0".to_string(),
            )])),
            catalogs: None,
        };

        let cat = ws.catalog.as_ref().unwrap();
        let cats = &HashMap::new();

        let deps = collect_member_deps_with_catalog(&ws, root.path(), cat, cats);
        assert!(!deps.is_empty(), "expected expanded deps");
        let react_dep = deps.iter().find(|d| d.name == "react").unwrap();
        assert_eq!(react_dep.version.as_deref(), Some("^19.0.0"));
    }

    #[test]
    fn test_collect_member_deps_with_catalog_override_warning() {
        let root = tempfile::tempdir().unwrap();
        let packages = root.path().join("packages");
        let member_dir = packages.join("pkg-a");
        std::fs::create_dir_all(&member_dir).unwrap();

        let member_json = r#"{
            "name": "pkg-a",
            "version": "0.1.0",
            "dependencies": {
                "react": "^18.0.0"
            }
        }"#;
        std::fs::write(member_dir.join("package.json"), member_json).unwrap();

        let ws = ara_manifest::types::Workspace {
            members: vec!["packages/*".to_string()],
            catalog: Some(HashMap::from([(
                "react".to_string(),
                "^19.0.0".to_string(),
            )])),
            catalogs: None,
        };

        let cat = ws.catalog.as_ref().unwrap();
        let cats = &HashMap::new();

        let deps = collect_member_deps_with_catalog(&ws, root.path(), cat, cats);
        let react_dep = deps.iter().find(|d| d.name == "react").unwrap();
        assert_eq!(react_dep.version.as_deref(), Some("^18.0.0"));
    }

    #[test]
    fn test_collect_member_deps_with_catalog_empty_member() {
        let root = tempfile::tempdir().unwrap();
        let packages = root.path().join("packages");
        let member_dir = packages.join("pkg-a");
        std::fs::create_dir_all(&member_dir).unwrap();

        let member_json = r#"{"name": "pkg-a", "version": "0.1.0"}"#;
        std::fs::write(member_dir.join("package.json"), member_json).unwrap();

        let ws = ara_manifest::types::Workspace {
            members: vec!["packages/*".to_string()],
            catalog: Some(HashMap::from([(
                "react".to_string(),
                "^19.0.0".to_string(),
            )])),
            catalogs: None,
        };

        let cat = ws.catalog.as_ref().unwrap();
        let cats = &HashMap::new();

        let deps = collect_member_deps_with_catalog(&ws, root.path(), cat, cats);
        assert!(deps.is_empty(), "expected no deps from empty member");
    }

    #[test]
    fn test_collect_member_deps_with_catalog_missing_package() {
        let root = tempfile::tempdir().unwrap();
        let packages = root.path().join("packages");
        let member_dir = packages.join("pkg-a");
        std::fs::create_dir_all(&member_dir).unwrap();

        let member_json = r#"{
            "name": "pkg-a",
            "version": "0.1.0",
            "dependencies": {
                "nonexistent": "catalog:"
            }
        }"#;
        std::fs::write(member_dir.join("package.json"), member_json).unwrap();

        let ws = ara_manifest::types::Workspace {
            members: vec!["packages/*".to_string()],
            catalog: Some(HashMap::from([(
                "react".to_string(),
                "^19.0.0".to_string(),
            )])),
            catalogs: None,
        };

        let cat = ws.catalog.as_ref().unwrap();
        let cats = &HashMap::new();

        let deps = collect_member_deps_with_catalog(&ws, root.path(), cat, cats);
        assert!(deps.is_empty(), "expected no deps from failed resolution");
    }
}
