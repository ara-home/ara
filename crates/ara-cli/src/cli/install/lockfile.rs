use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

use ara_lockfile::types::{GraphMeta, Lockfile, PackageEntry};
use ara_store::cas::Store;

pub(crate) fn write_lockfile(
    cwd: &Path,
    store: Option<&Store>,
    pkg_entries: &[PackageEntry],
    workspace_catalog: Option<&ara_manifest::types::Workspace>,
) -> Result<()> {
    let ts = current_timestamp();

    let graph_hash = if let Some(store) = store {
        if pkg_entries.is_empty() {
            None
        } else {
            let graph_bytes = serde_json::to_vec(pkg_entries)
                .context("failed to serialize package entries for graph hash")?;
            let raw = ara_util::hash::compute(&graph_bytes);
            let hex = ara_util::hash::hex_encode(&raw);
            let store_hash = store
                .put_graph(&graph_bytes)
                .context("failed to store graph hash in content store")?;
            Some(format!("sha256:{hex} (store: {store_hash})"))
        }
    } else {
        None
    };

    let lockfile_workspace = workspace_catalog.and_then(|ws| {
        if ws.catalog.is_none() && ws.catalogs.is_none() {
            None
        } else {
            Some(ara_lockfile::types::LockfileWorkspace {
                catalog: ws.catalog.clone(),
                catalogs: ws.catalogs.clone(),
            })
        }
    });

    let lockfile = Lockfile {
        version: 1,
        graph: GraphMeta {
            resolver: "mvs".to_string(),
            generated_at: Some(ts),
            graph_hash,
        },
        workspace: lockfile_workspace,
        packages: pkg_entries.to_vec(),
    };
    let lock_content = ara_lockfile::generator::generate(&lockfile);
    let lock_path = cwd.join("ara.lock");

    // Atomic write: write to temp file, then rename
    let tmp_path = cwd.join(format!("ara.lock.tmp.{}", uuid::Uuid::new_v4()));
    let mut tmp_f = std::fs::File::create(&tmp_path)?;
    tmp_f.write_all(lock_content.as_bytes())?;
    tmp_f.sync_all()?;
    std::fs::rename(&tmp_path, &lock_path)?;

    println!("Lockfile written to ara.lock");
    Ok(())
}

fn sha256_hex_to_sri(hash: &str) -> Option<String> {
    let hex = hash.strip_prefix("sha256-")?;
    if hex.len() != 64 {
        return None;
    }
    let bytes = hex::decode(hex).ok()?;
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("sha256-{b64}"))
}

fn npm_tarball_url(name: &str, version: &str) -> String {
    if let Some(scope) = name.strip_prefix('@') {
        if let Some((scope_name, rest)) = scope.split_once('/') {
            format!(
                "https://registry.npmjs.org/@{}%2f{}/-/@{}/{}-{}.tgz",
                scope_name, rest, scope_name, rest, version
            )
        } else {
            format!(
                "https://registry.npmjs.org/{}/-/{}-{}.tgz",
                name, name, version
            )
        }
    } else {
        format!(
            "https://registry.npmjs.org/{}/-/{}-{}.tgz",
            name, name, version
        )
    }
}

pub(crate) fn write_package_lock(
    cwd: &Path,
    manifest: &ara_manifest::types::Manifest,
    pkg_entries: &[PackageEntry],
) -> Result<()> {
    let mut packages = serde_json::Map::new();

    let root_deps: serde_json::Map<String, serde_json::Value> = manifest
        .deps
        .iter()
        .filter_map(|d| {
            let v = d.version.as_deref()?;
            Some((d.name.clone(), serde_json::Value::String(v.to_string())))
        })
        .collect();

    let mut root_entry = serde_json::Map::new();
    root_entry.insert(
        "name".to_string(),
        serde_json::Value::String(manifest.project.name.clone()),
    );
    root_entry.insert(
        "version".to_string(),
        serde_json::Value::String(manifest.project.version.clone()),
    );
    if !root_deps.is_empty() {
        root_entry.insert(
            "dependencies".to_string(),
            serde_json::Value::Object(root_deps),
        );
    }
    packages.insert(String::new(), serde_json::Value::Object(root_entry));

    let registry_url = std::env::var("ARA_NPM_REGISTRY")
        .unwrap_or_else(|_| "https://registry.npmjs.org".to_string());

    for entry in pkg_entries {
        let mut pkg = serde_json::Map::new();
        pkg.insert(
            "version".to_string(),
            serde_json::Value::String(entry.version.clone()),
        );

        if entry.source == "npm" || entry.source == "registry" {
            let resolved = if registry_url == "https://registry.npmjs.org" {
                npm_tarball_url(&entry.name, &entry.version)
            } else {
                format!(
                    "{}/{}/-/{}:{}.tgz",
                    registry_url.trim_end_matches('/'),
                    url_encode_pkg_name(&entry.name),
                    &entry.name,
                    entry.version
                )
            };
            pkg.insert("resolved".to_string(), serde_json::Value::String(resolved));

            if let Some(sri) = sha256_hex_to_sri(&entry.package_hash) {
                pkg.insert("integrity".to_string(), serde_json::Value::String(sri));
            }
        }

        let key = format!("node_modules/{}", entry.name);
        packages.insert(key, serde_json::Value::Object(pkg));
    }

    let lock = serde_json::json!({
        "name": manifest.project.name,
        "version": manifest.project.version,
        "lockfileVersion": 3,
        "requires": true,
        "packages": packages,
    });

    let output = serde_json::to_string_pretty(&lock)?;
    let lock_path = cwd.join("package-lock.json");
    std::fs::write(&lock_path, &output)?;
    println!("Compatibility lockfile written to package-lock.json");
    Ok(())
}

fn url_encode_pkg_name(name: &str) -> String {
    let mut encoded = String::with_capacity(name.len());
    for b in name.bytes() {
        match b {
            b'@' => encoded.push_str("%40"),
            b'/' => encoded.push_str("%2f"),
            _ => encoded.push(b as char),
        }
    }
    encoded
}

pub(crate) fn current_timestamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_current_timestamp_format() {
        let ts = current_timestamp();
        assert_eq!(ts.len(), 20, "expected ISO 8601 length, got: {ts}");
        assert_eq!(&ts[4..5], "-", "expected - after year: {ts}");
        assert_eq!(&ts[7..8], "-", "expected - after month: {ts}");
        assert_eq!(&ts[10..11], "T", "expected T separator: {ts}");
        assert_eq!(&ts[13..14], ":", "expected : after hour: {ts}");
        assert_eq!(&ts[16..17], ":", "expected : after minute: {ts}");
        assert_eq!(&ts[19..20], "Z", "expected Z suffix: {ts}");
    }

    #[test]
    fn test_current_timestamp_parses_as_date() {
        let ts = current_timestamp();
        let year: i64 = ts[0..4].parse().unwrap();
        let month: u32 = ts[5..7].parse().unwrap();
        let day: u32 = ts[8..10].parse().unwrap();
        assert!(year >= 2024, "year should be >= 2024, got {year}");
        assert!((1..=12).contains(&month), "month out of range: {month}");
        assert!((1..=31).contains(&day), "day out of range: {day}");
    }

    #[test]
    fn test_sha256_hex_to_sri() {
        let hash = "sha256-e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let result = sha256_hex_to_sri(hash);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="
        );
        assert!(sha256_hex_to_sri("sha256-tooshort").is_none());
        assert!(sha256_hex_to_sri("md5-abc").is_none());
    }

    #[test]
    fn test_npm_tarball_url_basic() {
        let url = npm_tarball_url("zod", "3.23.8");
        assert_eq!(url, "https://registry.npmjs.org/zod/-/zod-3.23.8.tgz");
    }

    #[test]
    fn test_npm_tarball_url_scoped() {
        let url = npm_tarball_url("@types/node", "25.9.2");
        assert_eq!(
            url,
            "https://registry.npmjs.org/@types%2fnode/-/@types/node-25.9.2.tgz"
        );
    }

    #[test]
    fn test_url_encode_pkg_name() {
        assert_eq!(url_encode_pkg_name("zod"), "zod");
        assert_eq!(url_encode_pkg_name("@types/node"), "%40types%2fnode");
        assert_eq!(url_encode_pkg_name("@scope/pkg"), "%40scope%2fpkg");
    }
}
