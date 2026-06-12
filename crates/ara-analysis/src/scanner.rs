use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const MAX_FILE_SIZE: u64 = 1_048_576; // 1 MB

const IGNORED_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".svn",
    ".hg",
    "dist",
    "target",
    "build",
    ".next",
    ".cache",
    "__pycache__",
];

const SOURCE_EXTENSIONS: &[&str] = &["js", "ts", "jsx", "tsx", "mjs", "cjs", "mts", "cts"];

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub content: String,
}

fn is_source_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext))
}

fn is_package_json(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "package.json")
}

fn is_declaration_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".d.ts") || n.ends_with(".d.mts") || n.ends_with(".d.cts"))
}

fn is_relevant_file(path: &Path) -> bool {
    (is_source_file(path) || is_package_json(path)) && !is_declaration_file(path)
}

fn is_ignored_dir(component: &str) -> bool {
    IGNORED_DIRS.contains(&component)
}

pub fn scan_package(package_path: &Path) -> Result<Vec<ScannedFile>> {
    if !package_path.exists() {
        anyhow::bail!("path does not exist: {}", package_path.display());
    }
    if !package_path.is_dir() {
        anyhow::bail!("path is not a directory: {}", package_path.display());
    }

    let mut files: Vec<ScannedFile> = Vec::new();

    let walker = walkdir::WalkDir::new(package_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            entry
                .file_name()
                .to_str()
                .is_none_or(|name| !is_ignored_dir(name))
        });

    for entry in walker {
        let entry = entry.context("failed to read directory entry")?;

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        if !is_relevant_file(path) {
            continue;
        }

        if entry
            .metadata()
            .ok()
            .is_some_and(|m| m.len() > MAX_FILE_SIZE)
        {
            continue;
        }

        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read file: {}", path.display()))?;

        if content.contains('\0') {
            continue;
        }

        files.push(ScannedFile {
            path: path.to_path_buf(),
            content,
        });
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use std::fs;
    use std::io::Write;

    fn create_temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("failed to create temp dir")
    }

    fn write_file(dir: &Path, rel: &str, content: &str) {
        let full = dir.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(&full).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn test_scanner_finds_js_ts_files() {
        let dir = create_temp_dir();
        write_file(dir.path(), "index.js", "console.log('hello')");
        write_file(dir.path(), "lib.ts", "export const x = 1;");
        write_file(dir.path(), "component.tsx", "export default () => {};");
        write_file(dir.path(), "styles.jsx", "const s = {};");
        write_file(dir.path(), "readme.md", "# docs");
        write_file(dir.path(), "main.py", "print('hello')");

        let files = scan_package(dir.path()).unwrap();
        let paths: Vec<&str> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_str().unwrap())
            .collect();

        assert!(paths.contains(&"index.js"));
        assert!(paths.contains(&"lib.ts"));
        assert!(paths.contains(&"component.tsx"));
        assert!(paths.contains(&"styles.jsx"));
        assert!(!paths.contains(&"readme.md"));
        assert!(!paths.contains(&"main.py"));
    }

    #[test]
    fn test_scanner_finds_package_json() {
        let dir = create_temp_dir();
        write_file(dir.path(), "package.json", "{}");
        write_file(dir.path(), "index.js", "console.log('x')");

        let files = scan_package(dir.path()).unwrap();
        let paths: Vec<&str> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(paths.contains(&"package.json"));
    }

    #[test]
    fn test_scanner_ignores_node_modules() {
        let dir = create_temp_dir();
        write_file(dir.path(), "index.js", "ok");
        write_file(dir.path(), "node_modules/evil/index.js", "eval(code)");
        write_file(dir.path(), "node_modules/package.json", "{}");

        let files = scan_package(dir.path()).unwrap();
        let paths: Vec<&str> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(paths.contains(&"index.js"));
        assert_eq!(paths.len(), 1);
    }

    #[test]
    fn test_scanner_ignores_git_dist_target() {
        let dir = create_temp_dir();
        write_file(dir.path(), "src/index.js", "ok");
        write_file(dir.path(), ".git/config", "ok");
        write_file(dir.path(), "dist/bundle.js", "compiled");
        write_file(dir.path(), "target/debug/output.js", "build");
        write_file(dir.path(), "build/out.js", "build");

        let files = scan_package(dir.path()).unwrap();
        let paths: Vec<&str> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(paths, vec!["index.js"]);
    }

    #[test]
    fn test_scanner_skips_binary_files() {
        let dir = create_temp_dir();
        write_file(dir.path(), "normal.js", "ok");
        // Write a file with null bytes
        let binary_path = dir.path().join("binary.js");
        fs::write(&binary_path, b"ok\x00binary").unwrap();

        let files = scan_package(dir.path()).unwrap();
        let paths: Vec<&str> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(paths.contains(&"normal.js"));
        assert!(!paths.contains(&"binary.js"));
    }

    #[test]
    fn test_scanner_skips_large_files() {
        let dir = create_temp_dir();
        write_file(dir.path(), "small.js", "console.log('ok')");
        let large_path = dir.path().join("large.js");
        let large_content = "x".repeat((MAX_FILE_SIZE as usize) + 1);
        fs::write(&large_path, large_content).unwrap();

        let files = scan_package(dir.path()).unwrap();
        let paths: Vec<&str> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(paths.contains(&"small.js"));
        assert!(!paths.contains(&"large.js"));
    }

    #[test]
    fn test_scanner_nonexistent_path() {
        let result = scan_package(Path::new("/tmp/nonexistent_ara_test_dir_12345"));
        assert!(result.is_err());
    }

    #[test]
    fn test_scanner_file_not_directory() {
        let dir = create_temp_dir();
        write_file(dir.path(), "file.js", "content");
        let result = scan_package(&dir.path().join("file.js"));
        assert!(result.is_err());
    }

    #[test]
    fn test_scanner_ignores_cache_svn_hg() {
        let dir = create_temp_dir();
        write_file(dir.path(), "src/index.js", "ok");
        write_file(dir.path(), ".cache/foo.js", "cached");
        write_file(dir.path(), ".svn/bar.js", "svn");
        write_file(dir.path(), ".hg/baz.js", "hg");
        write_file(dir.path(), "__pycache__/qux.js", "pyc");

        let files = scan_package(dir.path()).unwrap();
        let paths: Vec<&str> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(paths, vec!["index.js"]);
    }

    #[test]
    fn test_scanner_ignores_next_dir() {
        let dir = create_temp_dir();
        write_file(dir.path(), "index.js", "ok");
        write_file(dir.path(), ".next/bundle.js", "bundled");

        let files = scan_package(dir.path()).unwrap();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn test_scanner_ignores_d_ts() {
        let dir = create_temp_dir();
        write_file(dir.path(), "index.js", "ok");
        write_file(
            dir.path(),
            "types.d.ts",
            "declare function exec(cmd: string): void;",
        );
        write_file(
            dir.path(),
            "module.d.mts",
            "export function exec(cmd: string): void;",
        );
        write_file(
            dir.path(),
            "module.d.cts",
            "export function exec(cmd: string): void;",
        );

        let files = scan_package(dir.path()).unwrap();
        let paths: Vec<&str> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(paths.contains(&"index.js"));
        assert!(!paths.contains(&"types.d.ts"));
        assert!(!paths.contains(&"module.d.mts"));
        assert!(!paths.contains(&"module.d.cts"));
        assert_eq!(paths.len(), 1);
    }

    #[test]
    fn test_scanner_finds_mjs_cjs() {
        let dir = create_temp_dir();
        write_file(dir.path(), "module.mjs", "export const x = 1;");
        write_file(dir.path(), "module.cjs", "module.exports = {};");
        write_file(dir.path(), "module.mts", "export const y: number = 2;");
        write_file(dir.path(), "module.cts", "module.exports = {};");

        let files = scan_package(dir.path()).unwrap();
        let paths: Vec<&str> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(paths.contains(&"module.mjs"));
        assert!(paths.contains(&"module.cjs"));
        assert!(paths.contains(&"module.mts"));
        assert!(paths.contains(&"module.cts"));
    }

    #[test]
    fn test_scanner_handles_broken_symlink() {
        let dir = create_temp_dir();
        write_file(dir.path(), "real.js", "console.log('ok')");
        let broken = dir.path().join("broken.js");
        let _ = std::os::unix::fs::symlink("/nonexistent_target", &broken);

        let files = scan_package(dir.path()).unwrap();
        let paths: Vec<&str> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(paths.contains(&"real.js"));
    }

    #[test]
    fn test_scanner_handles_symlink_escape() {
        let dir = create_temp_dir();
        write_file(dir.path(), "real.js", "console.log('ok')");
        let outside = dir.path().join("escape.js");
        let _ = std::os::unix::fs::symlink("/etc/passwd", &outside);

        let files = scan_package(dir.path()).unwrap();
        let paths: Vec<&str> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert!(paths.contains(&"real.js"));
    }

    #[test]
    fn test_scanner_skips_symlink_dir() {
        let dir = create_temp_dir();
        write_file(dir.path(), "real.js", "ok");
        // Create target OUTSIDE the scanned directory
        let target = tempfile::tempdir().unwrap();
        write_file(target.path(), "inner.js", "should_not_appear");
        let link = dir.path().join("link_dir");
        let _ = std::os::unix::fs::symlink(target.path(), &link);

        let files = scan_package(dir.path()).unwrap();
        let paths: Vec<&str> = files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(paths, vec!["real.js"]);
    }

    #[test]
    fn test_scanner_many_files() {
        let dir = create_temp_dir();
        // Create 2000 small JS files to stress-test the scanner
        for i in 0..2000 {
            let name = format!("batch/file_{i:06}.js");
            write_file(dir.path(), &name, &format!("const x = {i};\n"));
        }
        // Add one real relevant file at root
        write_file(dir.path(), "index.js", "console.log('ok')");

        let files = scan_package(dir.path()).unwrap();
        assert_eq!(files.len(), 2001);
    }

    #[test]
    fn test_scanner_very_large_file_skipped() {
        let dir = create_temp_dir();
        write_file(dir.path(), "small.js", "ok");
        // Create a file significantly over the 1 MB limit
        // Using 2 MB to represent a "2 GB" scenario
        let huge_path = dir.path().join("huge.js");
        let huge_content = vec![b'x'; 2_000_000];
        std::fs::write(&huge_path, huge_content).unwrap();

        let files = scan_package(dir.path()).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(
            files[0].path.file_name().unwrap().to_str().unwrap(),
            "small.js"
        );
    }

    #[cfg(feature = "nightly-bench")]
    fn create_bench_scan_dir(n: usize) -> tempfile::TempDir {
        let dir = create_temp_dir();
        for i in 0..n {
            let name = format!("src/file_{i:06}.js");
            write_file(dir.path(), &name, &format!("const x = {i};\n"));
        }
        // Add some ignored dirs with files that should be skipped
        write_file(dir.path(), "node_modules/evil.js", "bad");
        write_file(dir.path(), ".git/config.js", "skip");
        write_file(dir.path(), "dist/bundle.js", "nope");
        dir
    }

    #[cfg(feature = "nightly-bench")]
    #[bench]
    fn bench_scan_100(b: &mut test::Bencher) {
        let dir = create_bench_scan_dir(100);
        b.iter(|| scan_package(test::black_box(dir.path())).unwrap());
    }

    #[cfg(feature = "nightly-bench")]
    #[bench]
    fn bench_scan_1000(b: &mut test::Bencher) {
        let dir = create_bench_scan_dir(1000);
        b.iter(|| scan_package(test::black_box(dir.path())).unwrap());
    }
}
