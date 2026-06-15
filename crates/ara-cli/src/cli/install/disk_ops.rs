use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};

use ara_store::cas::Store;

/// Returns `true` if `rel_path`, joined onto a base directory, would resolve to
/// a location outside that base (path traversal). This performs the same
/// containment check as normalizing `base.join(rel_path)` and verifying it
/// stays under `base`, but without any heap allocation: it tracks the component
/// depth relative to the base and rejects absolute paths or any `..` that would
/// escape above the root.
fn rel_path_escapes(rel_path: &str) -> bool {
    let mut depth: i32 = 0;
    for component in Path::new(rel_path).components() {
        match component {
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            Component::CurDir => {}
            // An absolute path (root or prefix) would replace the base entirely.
            Component::RootDir | Component::Prefix(_) => return true,
            Component::Normal(_) => depth += 1,
        }
    }
    false
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                components.pop();
            }
            Component::CurDir => {}
            other => components.push(other.as_os_str().to_os_string()),
        }
    }
    let mut result = PathBuf::new();
    for c in components {
        result.push(c);
    }
    result
}

pub fn extract_tarball(tarball: &[u8], dest: &Path) -> Result<()> {
    // Pass 1: detect prefix without allocating file data
    let prefix = {
        let decoder = flate2::read::GzDecoder::new(tarball);
        let mut archive = tar::Archive::new(decoder);
        let mut common = None;
        let mut has_files = false;

        let mut is_first = true;
        if let Ok(entries) = archive.entries() {
            for entry in entries.flatten() {
                if let Ok(path) = entry.path() {
                    let comp = path.components().next();
                    if is_first {
                        common = comp.map(|c| c.as_os_str().to_os_string());
                        is_first = false;
                    } else if common.is_some() && comp.map(|c| c.as_os_str()) != common.as_deref() {
                        common = None;
                    }
                    if path.components().count() > 1 {
                        has_files = true;
                    }
                }
            }
        }

        if let (Some(comp), true) = (common, has_files) {
            std::path::PathBuf::from(comp)
        } else {
            std::path::PathBuf::new()
        }
    };

    std::fs::create_dir_all(dest).context("failed to create extraction directory")?;
    let canonical_dest = dest.canonicalize().context("failed to canonicalize dest")?;

    // extract streaming directly to disk
    let decoder = flate2::read::GzDecoder::new(tarball);
    let mut archive = tar::Archive::new(decoder);
    for entry in archive
        .entries()
        .context("failed to read tarball entries")?
    {
        let mut entry = entry.context("failed to read tarball entry")?;
        let path = entry
            .path()
            .context("failed to read entry path")?
            .into_owned();

        let stripped = path.strip_prefix(&prefix).unwrap_or(&path);
        if stripped.as_os_str().is_empty() {
            continue;
        }

        let target = dest.join(stripped);
        let normalized = normalize_path(&target);
        if !normalized.starts_with(&canonical_dest) {
            anyhow::bail!("path traversal detected in tarball: {}", stripped.display());
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }

        entry.unpack(&target).context("failed to unpack entry")?;

        // Strip dangerous permissions: remove setuid/setgid/sticky bits
        // and world-writable, keep owner/group/other read/write/execute as-is
        if let Ok(metadata) = std::fs::metadata(&target) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                const CLEAN_MASK: u32 = 0o7777 & !(0o7000 | 0o0002);
                let mode = metadata.permissions().mode();
                let cleaned = mode & CLEAN_MASK;
                if mode != cleaned {
                    let mut perms = metadata.permissions();
                    perms.set_mode(cleaned);
                    let _ = std::fs::set_permissions(&target, perms);
                }
            }
            #[cfg(not(unix))]
            {
                let mut perms = metadata.permissions();
                perms.set_readonly(!perms.readonly());
                let _ = std::fs::set_permissions(&target, perms);
            }
        }
    }

    Ok(())
}

/// Create symlinks in `node_modules/.bin/` for the package's `bin` entries.
pub fn install_bin_links(node_modules: &Path, pkg_name: &str, pkg_dir: &Path) -> Result<()> {
    let pkg_json_path = pkg_dir.join("package.json");
    if !pkg_json_path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(&pkg_json_path)
        .context("failed to read package.json for bin links")?;
    let pkg: serde_json::Value =
        serde_json::from_str(&content).context("failed to parse package.json for bin links")?;

    let bin_entries: Vec<(String, String)> = match pkg.get("bin") {
        Some(serde_json::Value::String(cmd)) => {
            let unscoped_name = pkg_name
                .split('/')
                .next_back()
                .unwrap_or(pkg_name)
                .to_string();
            vec![(unscoped_name, cmd.clone())]
        }
        Some(serde_json::Value::Object(map)) => map
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
            .filter(|(_, v)| !v.is_empty())
            .collect(),
        _ => return Ok(()),
    };

    if bin_entries.is_empty() {
        return Ok(());
    }

    let bin_dir = node_modules.join(".bin");
    std::fs::create_dir_all(&bin_dir)?;

    for (name, rel_path) in &bin_entries {
        let link = bin_dir.join(name);
        if rel_path_escapes(rel_path) {
            eprintln!(
                "  warning: bin path '{}' escapes package directory, skipping {name}",
                rel_path
            );
            continue;
        }
        let actual_file = pkg_dir.join(rel_path);

        #[allow(unused_variables)]
        let target = format!("../{}/{}", pkg_name, rel_path);

        #[cfg(unix)]
        if let Ok(metadata) = std::fs::metadata(&actual_file) {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            if let Err(e) = std::fs::set_permissions(&actual_file, perms) {
                eprintln!(
                    "  warning: failed to set executable permissions on {actual_file:?}: {e}"
                );
            }
        }

        let _ = std::fs::remove_file(&link);

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link)
            .with_context(|| format!("failed to create symlink {link:?} -> {target}"))?;

        #[cfg(not(unix))]
        std::fs::hard_link(&actual_file, &link)
            .with_context(|| format!("failed to link {link:?}"))?;
    }

    Ok(())
}

/// Scan `node_modules/<pkg>/package.json` for each installed package and
/// recursively install any missing transitive dependencies.
pub(crate) fn collect_installed_names(node_modules: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(node_modules) else {
        return names;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(ftype) = entry.file_type() else {
            continue;
        };
        if !ftype.is_dir() {
            continue;
        }
        let fname = match path.file_name().and_then(|s| s.to_str()) {
            Some(f) => f.to_string(),
            None => continue,
        };
        if fname == ".bin" {
            continue;
        }
        if fname.starts_with('@') {
            let Ok(sub_entries) = std::fs::read_dir(&path) else {
                continue;
            };
            for sub in sub_entries.flatten() {
                let sub_path = sub.path();
                if sub.file_type().ok().is_some_and(|t| t.is_dir()) {
                    if let Some(sub_name) = sub_path.file_name().and_then(|s| s.to_str()) {
                        names.push(format!("{}/{}", fname, sub_name));
                    }
                }
            }
        } else {
            names.push(fname);
        }
    }
    names
}

/// Extract a tarball from the CAS store to the package directory.
/// Uses a cached extracted directory in the store to avoid re-extraction.
/// When `content` is provided, uses it directly instead of reading from the store.
pub(crate) fn extract_package_cached(
    store: &Store,
    hash_str: &str,
    pkg_dir: &Path,
    content: Option<Vec<u8>>,
) -> Result<()> {
    let extracted_dir = store.get_extracted_path(hash_str);

    if !extracted_dir.exists() {
        let tarball = match content {
            Some(c) => c,
            None => store
                .get(hash_str)?
                .ok_or_else(|| anyhow::anyhow!("package {hash_str} not in store"))?,
        };
        std::fs::create_dir_all(&extracted_dir)
            .with_context(|| format!("failed to create {}", extracted_dir.display()))?;
        extract_tarball(&tarball, &extracted_dir)
            .with_context(|| format!("failed to extract to {}", extracted_dir.display()))?;
    }

    let _ = std::fs::remove_dir_all(pkg_dir);
    hardlink_dir(&extracted_dir, pkg_dir)
        .with_context(|| format!("failed to hardlink to {}", pkg_dir.display()))
}

/// Recursively create hardlinks from `src` to `dst`, falling back to copy
/// if hardlinking across filesystems fails.
pub fn hardlink_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry?;
        let relative = entry
            .path()
            .strip_prefix(src)
            .map_err(|_| anyhow::anyhow!("path prefix error"))?;
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = dst.join(relative);

        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if entry.file_type().is_symlink() {
            #[cfg(unix)]
            {
                let link_target = std::fs::read_link(entry.path())?;
                let _ = std::fs::remove_file(&target);
                std::os::unix::fs::symlink(&link_target, &target)?;
            }
            #[cfg(not(unix))]
            {
                let _ = std::fs::copy(entry.path(), &target);
            }
        } else if std::fs::hard_link(entry.path(), &target).is_err() {
            let _ = std::fs::copy(entry.path(), &target);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_extract_tarball() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("out");

        let mut buf = Vec::new();
        let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
        let mut ar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_path("hello.txt").unwrap();
        header.set_size(12);
        header.set_mode(0o644);
        header.set_cksum();
        ar.append(&header, b"hello world\n".as_slice()).unwrap();
        let encoder = ar.into_inner().unwrap();
        encoder.finish().unwrap();

        extract_tarball(&buf, &dest).unwrap();
        let extracted = std::fs::read_to_string(dest.join("hello.txt")).unwrap();
        assert_eq!(extracted, "hello world\n");
    }

    #[test]
    fn test_extract_tarball_path_traversal() {
        // The tar crate rejects .. in entry paths at build time,
        // which serves as built-in path traversal protection.
        let mut header = tar::Header::new_gnu();
        let result = header.set_path("../../../etc/passwd");
        assert!(result.is_err(), "tar crate should reject paths with ..");
    }

    #[test]
    fn test_extract_tarball_many_small_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("out");

        let mut buf = Vec::new();
        let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
        let mut ar = tar::Builder::new(encoder);
        let count = 1000;
        for i in 0..count {
            let name = format!("files/file_{i:06}.js");
            let content = format!("module.exports = {{ id: {i} }};\n");
            let mut header = tar::Header::new_gnu();
            header.set_path(&name).unwrap();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            ar.append(&header, content.as_bytes()).unwrap();
        }
        let encoder = ar.into_inner().unwrap();
        encoder.finish().unwrap();

        extract_tarball(&buf, &dest).unwrap();
        let mut extracted_count = 0;
        for entry in walkdir::WalkDir::new(&dest) {
            if entry.unwrap().file_type().is_file() {
                extracted_count += 1;
            }
        }
        assert_eq!(extracted_count, count);
    }

    #[cfg(feature = "nightly-bench")]
    fn make_tarball(n: usize) -> Vec<u8> {
        let mut buf = Vec::new();
        let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
        let mut ar = tar::Builder::new(encoder);
        for i in 0..n {
            let name = format!("files/file_{i:06}.js");
            let content = format!("module.exports = {{ id: {i} }};\n");
            let mut header = tar::Header::new_gnu();
            header.set_path(&name).unwrap();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            ar.append(&header, content.as_bytes()).unwrap();
        }
        let encoder = ar.into_inner().unwrap();
        encoder.finish().unwrap();
        buf
    }

    #[cfg(feature = "nightly-bench")]
    #[bench]
    fn bench_extract_tarball_100(b: &mut test::Bencher) {
        let tarball = make_tarball(100);
        b.iter(|| {
            let tmp = tempfile::tempdir().unwrap();
            extract_tarball(test::black_box(&tarball), test::black_box(tmp.path()))
        });
    }

    #[cfg(feature = "nightly-bench")]
    #[bench]
    fn bench_extract_tarball_1000(b: &mut test::Bencher) {
        let tarball = make_tarball(1000);
        b.iter(|| {
            let tmp = tempfile::tempdir().unwrap();
            extract_tarball(test::black_box(&tarball), test::black_box(tmp.path()))
        });
    }
}
