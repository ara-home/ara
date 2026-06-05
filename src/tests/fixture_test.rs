use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// Locate the `ara` binary built by Cargo.
fn ara_binary() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_ara") {
        return PathBuf::from(path);
    }
    let test_exe = std::env::current_exe().expect("failed to get test binary path");
    let mut dir = test_exe.parent().expect("test binary has no parent");
    if dir.ends_with("deps") {
        dir = dir.parent().expect("deps dir has no parent");
    }
    dir.join("ara")
}

/// Recursively copy a directory tree.
fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("failed to create dest dir");
    for entry in std::fs::read_dir(src).expect("failed to read source dir") {
        let entry = entry.expect("failed to read entry");
        let ty = entry.file_type().expect("failed to get file type");
        let name = entry.file_name();
        let src_path = entry.path();
        let dst_path = dst.join(&name);
        if ty.is_dir() {
            copy_dir(&src_path, &dst_path);
        } else {
            std::fs::copy(&src_path, &dst_path).expect("failed to copy file");
        }
    }
}

/// Build a minimal gzipped tarball with a single file.
fn make_minimal_tarball() -> Vec<u8> {
    let mut buf = Vec::new();
    let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
    let mut ar = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    header.set_path("index.js").unwrap();
    header.set_size(13);
    header.set_mode(0o644);
    header.set_cksum();
    ar.append(&header, "module.exports = {};\n".as_bytes())
        .unwrap();
    let encoder = ar.into_inner().unwrap();
    encoder.finish().unwrap();
    buf
}

/// Register mockito mocks for an npm package.
/// Returns the mocks to keep them alive for the duration of the test.
#[allow(clippy::needless_pass_by_value)]
fn mock_npm_package(server: &mut mockito::Server, name: &str, version: &str) -> Vec<mockito::Mock> {
    let clean_ver = version
        .trim_start_matches('^')
        .trim_start_matches('~')
        .trim_start_matches('>')
        .trim_start_matches('<')
        .trim_start_matches('=')
        .trim_start_matches('*');

    // Versions endpoint:  GET /{name}
    let versions_body = serde_json::json!({
        "name": name,
        "versions": {
            clean_ver: {
                "name": name,
                "version": clean_ver,
                "dist": {
                    "tarball": format!("{}/{}/-/{}-{}.tgz", server.url(), name, name, clean_ver)
                }
            }
        }
    });
    let m1 = server
        .mock("GET", format!("/{name}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(versions_body.to_string())
        .create();

    // Tarball endpoint:  GET /{name}/-/{name}-{version}.tgz
    let tarball = make_minimal_tarball();
    let m2 = server
        .mock("GET", format!("/{name}/-/{name}-{clean_ver}.tgz").as_str())
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .with_body(tarball)
        .create();

    vec![m1, m2]
}

/// Extract a simple value from a TOML-like line: `key = "value"`.
fn extract_toml_value<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    let pattern = &format!("{field} = \"");
    let start = line.find(pattern)?;
    let rest = &line[start + pattern.len()..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// Scan a fixture directory for npm dependencies by reading manifest files.
fn find_npm_deps(fixture_dir: &Path) -> Vec<(String, String)> {
    let mut deps = Vec::new();

    // Try ara.toml
    let ara_toml = fixture_dir.join("ara.toml");
    if ara_toml.exists() {
        let content = std::fs::read_to_string(&ara_toml).ok();
        if let Some(c) = content {
            let mut in_deps = false;
            for line in c.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') {
                    in_deps = trimmed == "[deps]";
                    continue;
                }
                if !in_deps || trimmed.is_empty() {
                    continue;
                }
                // Line format: key = { source = "...", version = "..." }
                if let Some(eq_pos) = trimmed.find('=') {
                    let key = trimmed[..eq_pos].trim().trim_matches('"');
                    if let Some(source) = extract_toml_value(trimmed, "source") {
                        if source == "npm" || source == "registry" {
                            let ver = extract_toml_value(trimmed, "version").unwrap_or("*");
                            deps.push((key.to_string(), ver.to_string()));
                        }
                    }
                }
            }
        }
    }

    // Try package.json
    let pkg_json = fixture_dir.join("package.json");
    if pkg_json.exists() {
        let content = std::fs::read_to_string(&pkg_json).ok();
        if let Some(c) = content {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&c) {
                for field in &[
                    "dependencies",
                    "devDependencies",
                    "peerDependencies",
                    "optionalDependencies",
                ] {
                    if let Some(deps_map) = val.get(field).and_then(|v| v.as_object()) {
                        for (k, v) in deps_map {
                            let ver = v.as_str().unwrap_or("*");
                            deps.push((k.clone(), ver.to_string()));
                        }
                    }
                }
            }
        }
    }

    deps.sort();
    deps.dedup_by(|a, b| a.0 == b.0);
    deps
}

/// Result of running a single fixture scenario.
#[derive(Debug)]
struct FixtureResult {
    name: String,
    passed: bool,
    duration_ms: u64,
    detail: String,
}

fn run_fixture(fixture_root: &Path, category: &str, name: &str) -> FixtureResult {
    let start = Instant::now();

    // Start mockito server for this fixture
    let mut server = mockito::Server::new();
    let registry_url = server.url();
    server.mock("GET", "/favicon.ico").with_status(404).create();

    let fixture_dir = fixture_root.join(category).join(name);
    let npm_deps = find_npm_deps(&fixture_dir);
    let mut _mocks = Vec::new();
    for (pkg, ver) in &npm_deps {
        _mocks.extend(mock_npm_package(&mut server, pkg, ver));
    }

    let tmp = tempfile::tempdir().expect("failed to create tempdir");
    let project_dir = tmp.path().join("project");
    copy_dir(&fixture_dir, &project_dir);

    let bin = ara_binary();
    let output = Command::new(&bin)
        .args(["install", "--non-interactive"])
        .current_dir(&project_dir)
        .env("ARA_NPM_REGISTRY", &registry_url)
        .output()
        .expect("failed to run ara install");

    let duration_ms = start.elapsed().as_millis() as u64;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let lockfile = project_dir.join("ara.lock");
    let lockfile_exists = lockfile.exists();

    let (should_succeed, check_lockfile) = match category {
        "valid" | "edge" => (true, true),
        "malformed" => (false, false),
        _ => (true, false),
    };

    let success = output.status.success();
    let passed = if should_succeed {
        success && (!check_lockfile || lockfile_exists)
    } else {
        !success
    };

    let detail = if passed {
        if should_succeed {
            format!(
                "exit=0 lockfile={lockfile_exists} {duration_ms}ms deps={}",
                npm_deps.len()
            )
        } else {
            format!(
                "exit={} {duration_ms}ms (expected failure)",
                output.status.code().unwrap_or(-1)
            )
        }
    } else {
        let mut msg = format!(
            "exit={} {duration_ms}ms",
            output.status.code().unwrap_or(-1)
        );
        if let Some(line) = stdout.trim().lines().next().filter(|l| !l.is_empty()) {
            msg.push_str(&format!("\n  stdout: {line}"));
        }
        if let Some(line) = stderr.trim().lines().next().filter(|l| !l.is_empty()) {
            msg.push_str(&format!("\n  stderr: {line}"));
        }
        if check_lockfile && !lockfile_exists {
            msg.push_str("\n  ara.lock was not created");
        }
        msg
    };

    FixtureResult {
        name: format!("{category}/{name}"),
        passed,
        duration_ms,
        detail,
    }
}

fn discover_fixtures(fixtures_root: &Path, category: &str) -> Vec<String> {
    let dir = fixtures_root.join(category);
    if !dir.exists() {
        return vec![];
    }
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("failed to read fixture dir")
        .filter_map(|e| {
            let e = e.ok()?;
            if e.file_type().ok()?.is_dir() {
                Some(e.file_name().to_str()?.to_string())
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names
}

#[test]
fn test_fixtures_valid() {
    let fixtures_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let names = discover_fixtures(&fixtures_root, "valid");
    assert!(!names.is_empty(), "no valid/ fixtures found");

    let mut results: Vec<FixtureResult> = Vec::new();
    for name in &names {
        let result = run_fixture(&fixtures_root, "valid", name);
        results.push(result);
    }

    print_summary(&results);
    let failures: Vec<_> = results.iter().filter(|r| !r.passed).collect();
    assert!(
        failures.is_empty(),
        "{} valid fixture(s) failed:\n{}",
        failures.len(),
        failures
            .iter()
            .map(|r| format!("  {}: {}", r.name, r.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn print_summary(results: &[FixtureResult]) {
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.len() - passed;
    println!(
        "\n  fixtures  {}/{} passed ({} failed)",
        passed,
        results.len(),
        failed
    );
    for r in results {
        let icon = if r.passed { "ok" } else { "FAIL" };
        println!("    {icon:4}  {}  {}ms", r.name, r.duration_ms);
    }
}
