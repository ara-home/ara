use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

type CatalogMap = HashMap<String, String>;
type CatalogsMap = HashMap<String, HashMap<String, String>>;

/// Show workspace catalog entries.
pub fn cmd_catalog_list() -> Result<()> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    list_catalog_from(&cwd)
}

fn list_catalog_from(cwd: &Path) -> Result<()> {
    let (catalog, catalogs): (Option<CatalogMap>, Option<CatalogsMap>) =
        read_catalog_from_manifest(cwd)?;

    if let Some(ref cat) = catalog {
        if cat.is_empty() {
            println!("No entries in default catalog");
        } else {
            println!("[workspace.catalog]");
            for (name, constraint) in cat {
                println!("  {name} = \"{constraint}\"");
            }
        }
    } else {
        println!("No default catalog defined");
    }

    if let Some(ref cats) = catalogs {
        for (cat_name, entries) in cats {
            if entries.is_empty() {
                println!("  [workspace.catalogs.{cat_name}] (empty)");
            } else {
                println!("\n[workspace.catalogs.{cat_name}]");
                for (name, constraint) in entries {
                    println!("  {name} = \"{constraint}\"");
                }
            }
        }
    }

    Ok(())
}

/// Add an entry to the default workspace catalog (CLI entry point).
pub fn cmd_catalog_add(name: &str, version: &str) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    add_catalog_entry(&cwd, name, version)
}

fn add_catalog_entry(cwd: &Path, name: &str, version: &str) -> Result<()> {
    let manifest_path = cwd.join("ara.toml");

    let content = if manifest_path.exists() {
        std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?
    } else {
        String::new()
    };

    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    let catalog_section = "[workspace.catalog]";
    let catalog_idx = lines.iter().position(|l| l.trim() == catalog_section);

    if let Some(idx) = catalog_idx {
        let mut insert_pos = idx + 1;
        while insert_pos < lines.len() {
            let trimmed = lines[insert_pos].trim();
            if trimmed.starts_with('[') {
                break;
            }
            if let Some(eq_pos) = trimmed.find('=') {
                let existing_name = trimmed[..eq_pos].trim().trim_matches('"');
                if existing_name == name {
                    lines[insert_pos] = format!("{name} = \"{version}\"");
                    let out = lines.join("\n") + "\n";
                    write_manifest(&manifest_path, &out)?;
                    println!("Updated catalog entry {name} = \"{version}\"");
                    return Ok(());
                }
            }
            insert_pos += 1;
        }
        lines.insert(insert_pos, format!("{name} = \"{version}\""));
    } else if lines.iter().any(|l| l.trim() == "[workspace]") {
        let ws_idx = lines.iter().position(|l| l.trim() == "[workspace]");
        if let Some(ws_idx) = ws_idx {
            let mut insert_after = ws_idx + 1;
            while insert_after < lines.len() {
                let trimmed = lines[insert_after].trim();
                if trimmed.starts_with('[') {
                    break;
                }
                insert_after += 1;
            }
            if insert_after > 0 && !lines[insert_after - 1].is_empty() {
                lines.insert(insert_after, String::new());
                insert_after += 1;
            }
            lines.insert(insert_after, catalog_section.to_string());
            lines.insert(insert_after + 1, format!("{name} = \"{version}\""));
        }
    } else {
        if !lines.is_empty() && !lines[lines.len() - 1].is_empty() {
            lines.push(String::new());
        }
        lines.push("[workspace]".to_string());
        lines.push(String::new());
        lines.push(catalog_section.to_string());
        lines.push(format!("{name} = \"{version}\""));
    }

    let out = lines.join("\n") + "\n";
    write_manifest(&manifest_path, &out)?;
    println!("Added catalog entry {name} = \"{version}\"");
    Ok(())
}

/// Read catalog from manifest files using the manifest parser.
fn read_catalog_from_manifest(cwd: &Path) -> Result<(Option<CatalogMap>, Option<CatalogsMap>)> {
    let manifest_path = cwd.join("ara.toml");
    let pkg_json_path = cwd.join("package.json");

    let from_workspace = |ws: Option<ara_manifest::types::Workspace>| {
        ws.map(|w| (w.catalog, w.catalogs)).unwrap_or((None, None))
    };

    if manifest_path.exists() {
        let content = std::fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        if let Ok(m) = ara_manifest::parser::parse(&content) {
            return Ok(from_workspace(m.workspace));
        }
    }

    if pkg_json_path.exists() {
        let content = std::fs::read_to_string(&pkg_json_path)
            .with_context(|| format!("failed to read {}", pkg_json_path.display()))?;
        if let Ok(m) = ara_manifest::package_json::parse_package_json(&content) {
            return Ok(from_workspace(m.workspace));
        }
    }

    Ok((None, None))
}

fn write_manifest(path: &Path, content: &str) -> Result<()> {
    let mut f = std::fs::File::create(path).context("failed to write ara.toml")?;
    f.write_all(content.as_bytes())
        .context("failed to write ara.toml")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_cmd_catalog_add_creates_section() {
        let root = tempfile::tempdir().unwrap();
        add_catalog_entry(root.path(), "react", "^19.0.0").unwrap();

        let content = std::fs::read_to_string(root.path().join("ara.toml")).unwrap();
        assert!(content.contains("[workspace.catalog]"));
        assert!(content.contains("react = \"^19.0.0\""));
    }

    #[test]
    fn test_cmd_catalog_add_updates_existing() {
        let root = tempfile::tempdir().unwrap();

        std::fs::write(
            root.path().join("ara.toml"),
            r#"[workspace]
members = ["packages/*"]

[workspace.catalog]
react = "^18.0.0"
"#,
        )
        .unwrap();

        add_catalog_entry(root.path(), "react", "^19.0.0").unwrap();

        let content = std::fs::read_to_string(root.path().join("ara.toml")).unwrap();
        assert!(content.contains("react = \"^19.0.0\""));
        assert!(!content.contains("react = \"^18.0.0\""));
    }

    #[test]
    fn test_cmd_catalog_add_appends_to_existing_section() {
        let root = tempfile::tempdir().unwrap();

        std::fs::write(
            root.path().join("ara.toml"),
            r#"[workspace]
members = ["packages/*"]

[workspace.catalog]
react = "^19.0.0"
"#,
        )
        .unwrap();

        add_catalog_entry(root.path(), "react-dom", "^19.0.0").unwrap();

        let content = std::fs::read_to_string(root.path().join("ara.toml")).unwrap();
        assert!(content.contains("react-dom = \"^19.0.0\""));
    }

    #[test]
    fn test_read_catalog_from_ara_toml() {
        let root = tempfile::tempdir().unwrap();
        let content = r#"[project]
name = "test"
version = "1.0.0"

[workspace]
members = ["packages/*"]

[workspace.catalog]
react = "^19.0.0"
react-dom = "^19.0.0"
"#;
        std::fs::write(root.path().join("ara.toml"), content).unwrap();

        let (catalog, catalogs) = read_catalog_from_manifest(root.path()).unwrap();
        assert!(catalog.is_some());
        let cat = catalog.unwrap();
        assert_eq!(cat.get("react").unwrap(), "^19.0.0");
        assert_eq!(cat.get("react-dom").unwrap(), "^19.0.0");
        assert!(catalogs.is_none());
    }

    #[test]
    fn test_read_catalog_named_catalogs() {
        let root = tempfile::tempdir().unwrap();
        let content = r#"[project]
name = "test"
version = "1.0.0"

[workspace.catalog]
react = "^19.0.0"

[workspace.catalogs.testing]
jest = "30.0.0"
vitest = "^2.0.0"
"#;
        std::fs::write(root.path().join("ara.toml"), content).unwrap();

        let (catalog, catalogs) = read_catalog_from_manifest(root.path()).unwrap();
        assert!(catalog.is_some());
        let cat = catalog.unwrap();
        assert_eq!(cat.get("react").unwrap(), "^19.0.0");

        let cats = catalogs.unwrap();
        let testing = cats.get("testing").unwrap();
        assert_eq!(testing.get("jest").unwrap(), "30.0.0");
        assert_eq!(testing.get("vitest").unwrap(), "^2.0.0");
    }

    #[test]
    fn test_read_catalog_no_manifest() {
        let root = tempfile::tempdir().unwrap();
        let (_catalog, _catalogs) = read_catalog_from_manifest(root.path()).unwrap();
        assert!(_catalog.is_none());
        assert!(_catalogs.is_none());
    }

    #[test]
    fn test_read_catalog_from_package_json() {
        let root = tempfile::tempdir().unwrap();
        let content = r#"{
            "name": "test",
            "version": "1.0.0",
            "workspaces": {
                "members": ["packages/*"],
                "catalog": {
                    "react": "^19.0.0"
                }
            }
        }"#;
        std::fs::write(root.path().join("package.json"), content).unwrap();

        let (catalog, catalogs) = read_catalog_from_manifest(root.path()).unwrap();
        assert!(catalog.is_some());
        assert!(catalogs.is_none(), "no named catalogs in this fixture");
        let cat = catalog.unwrap();
        assert_eq!(cat.get("react").unwrap(), "^19.0.0");
    }
}
