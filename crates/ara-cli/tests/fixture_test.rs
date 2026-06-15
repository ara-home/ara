#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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

/// Build a gzipped tarball with custom files (path -> content).
fn make_tarball_with_files(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();
    let encoder = flate2::write::GzEncoder::new(&mut buf, flate2::Compression::fast());
    let mut ar = tar::Builder::new(encoder);
    for (path, content) in files {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        ar.append(&header, *content).unwrap();
    }
    let encoder = ar.into_inner().unwrap();
    encoder.finish().unwrap();
    buf
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
    let clean_ver = if clean_ver.is_empty() {
        "0.0.0"
    } else {
        clean_ver
    };

    // For scoped packages (@scope/name), the tarball filename uses only the bare name
    let bare_name = name.rsplit('/').next().unwrap_or(name);

    // Versions endpoint:  GET /{name}
    let versions_body = serde_json::json!({
        "name": name,
        "dist-tags": { "latest": clean_ver },
        "versions": {
            clean_ver: {
                "name": name,
                "version": clean_ver,
                "dist": {
                    "tarball": format!("{}/{}/-/{}-{}.tgz", server.url(), name, bare_name, clean_ver)
                }
            }
        }
    });
    let m1 = server
        .mock("GET", format!("/{name}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(versions_body.to_string())
        .expect_at_least(1)
        .create();

    // Tarball endpoint:  GET /{name}/-/{bare_name}-{version}.tgz
    let tarball = make_minimal_tarball();
    let m2 = server
        .mock(
            "GET",
            format!("/{name}/-/{bare_name}-{clean_ver}.tgz").as_str(),
        )
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .with_body(tarball)
        .create();

    vec![m1, m2]
}

/// Like `mock_npm_package`, but includes `dependencies` in the version metadata.
/// `deps` is a list of `(dep_name, dep_version)` pairs.
fn mock_npm_package_with_deps(
    server: &mut mockito::Server,
    name: &str,
    version: &str,
    deps: &[(&str, &str)],
) -> Vec<mockito::Mock> {
    let clean_ver = version
        .trim_start_matches('^')
        .trim_start_matches('~')
        .trim_start_matches('>')
        .trim_start_matches('<')
        .trim_start_matches('=')
        .trim_start_matches('*');
    let clean_ver = if clean_ver.is_empty() {
        "0.0.0"
    } else {
        clean_ver
    };

    let bare_name = name.rsplit('/').next().unwrap_or(name);

    let deps_map: serde_json::Map<String, serde_json::Value> = deps
        .iter()
        .map(|(k, v)| (k.to_string(), serde_json::Value::String(v.to_string())))
        .collect();
    let mut version_obj = serde_json::json!({
        "name": name,
        "version": clean_ver,
        "dist": {
            "tarball": format!("{}/{}/-/{}-{}.tgz", server.url(), name, bare_name, clean_ver)
        }
    });
    if !deps_map.is_empty() {
        version_obj["dependencies"] = serde_json::Value::Object(deps_map);
    }
    let versions_body = serde_json::json!({
        "name": name,
        "dist-tags": { "latest": clean_ver },
        "versions": { clean_ver: version_obj }
    });

    let m1 = server
        .mock("GET", format!("/{name}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(versions_body.to_string())
        .expect_at_least(1)
        .create();

    let tarball = make_minimal_tarball();
    let m2 = server
        .mock(
            "GET",
            format!("/{name}/-/{bare_name}-{clean_ver}.tgz").as_str(),
        )
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
                            if ver.starts_with("workspace:") {
                                continue;
                            }
                            deps.push((k.clone(), ver.to_string()));
                        }
                    }
                }
            }
        }
    }

    // Try .npm-deps file (used by URL install fixture scenarios)
    let npm_deps_file = fixture_dir.join(".npm-deps");
    if npm_deps_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&npm_deps_file) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                let parts: Vec<&str> = trimmed.splitn(2, char::is_whitespace).collect();
                let pkg_name = parts[0].trim();
                let pkg_ver = parts.get(1).unwrap_or(&"*").trim();
                if !pkg_name.is_empty() {
                    deps.push((pkg_name.to_string(), pkg_ver.to_string()));
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

/// Run a fixture using `ara install --non-interactive` and verify result.
fn run_fixture(fixture_root: &Path, category: &str, name: &str) -> FixtureResult {
    run_fixture_with_command(
        fixture_root,
        category,
        name,
        &["install", "--non-interactive"],
    )
}

/// Run a fixture using `ara analyze` for security pattern detection.
fn run_fixture_analyze(fixture_root: &Path, category: &str, name: &str) -> FixtureResult {
    run_fixture_with_command(fixture_root, category, name, &["analyze"])
}

fn run_fixture_with_command(
    fixture_root: &Path,
    category: &str,
    name: &str,
    args: &[&str],
) -> FixtureResult {
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
        .args(args)
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
        "valid" | "edge" | "workspace" => (true, true),
        "malformed" => (false, false),
        "" => (true, true), // URL install fixtures
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

fn run_test_category(
    fixtures_root: &Path,
    category: &str,
    runner: fn(&Path, &str, &str) -> FixtureResult,
) {
    let names = discover_fixtures(fixtures_root, category);
    if names.is_empty() {
        println!("\n  SKIP   0 fixtures in {category}/");
        return;
    }

    let mut results: Vec<FixtureResult> = Vec::new();
    for name in &names {
        let result = runner(fixtures_root, category, name);
        results.push(result);
    }

    print_summary(&results);
    let failures: Vec<_> = results.iter().filter(|r| !r.passed).collect();
    assert!(
        failures.is_empty(),
        "{} {category} fixture(s) failed:\n{}",
        failures.len(),
        failures
            .iter()
            .map(|r| format!("  {}: {}", r.name, r.detail))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn test_fixtures_valid() {
    let fixtures_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    run_test_category(&fixtures_root, "valid", run_fixture);
}

#[test]
fn test_fixtures_edge() {
    let fixtures_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    run_test_category(&fixtures_root, "edge", run_fixture);
}

#[test]
fn test_fixtures_malformed() {
    let fixtures_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    run_test_category(&fixtures_root, "malformed", run_fixture);
}

#[test]
fn test_fixtures_security() {
    let fixtures_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    run_test_category(&fixtures_root, "security", run_fixture_analyze);
}

#[test]
fn test_fixtures_workspace() {
    let fixtures_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    run_test_category(&fixtures_root, "workspace", run_fixture);
}

// ---------------------------------------------------------------------------
// URL install fixtures — each scenario defines its own args
// ---------------------------------------------------------------------------

#[test]
fn test_install_url_by_name() {
    let r = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let result = run_fixture_with_command(
        &r,
        "",
        "13-install-by-name",
        &["install", "--non-interactive", "zod"],
    );
    assert!(result.passed, "{}", result.detail);
}

#[test]
fn test_install_url_by_name_with_version() {
    let r = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let result = run_fixture_with_command(
        &r,
        "",
        "14-install-by-name-with-version",
        &["install", "--non-interactive", "zod@3.22.0"],
    );
    assert!(result.passed, "{}", result.detail);
}

#[test]
fn test_install_url_with_save_dev() {
    let r = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let result = run_fixture_with_command(
        &r,
        "",
        "15-install-with-save-dev",
        &["install", "--non-interactive", "--save-dev", "eslint"],
    );
    assert!(result.passed, "{}", result.detail);
}

#[test]
fn test_install_url_multiple() {
    let r = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let result = run_fixture_with_command(
        &r,
        "",
        "16-install-multiple",
        &["install", "--non-interactive", "react", "zod", "typescript"],
    );
    assert!(result.passed, "{}", result.detail);
}

#[test]
fn test_install_url_into_existing_manifest() {
    let r = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let result = run_fixture_with_command(
        &r,
        "",
        "17-install-into-existing-manifest",
        &["install", "--non-interactive", "express"],
    );
    assert!(result.passed, "{}", result.detail);
}

// ---------------------------------------------------------------------------
// Custom E2E fixture tests
// ---------------------------------------------------------------------------

#[test]
fn test_install_transitive_deps() {
    let r = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let fixture_dir = r.join("valid/13-transitive-deps");

    let mut server = mockito::Server::new();
    let registry_url = server.url();
    server.mock("GET", "/favicon.ico").with_status(404).create();

    // dep-a@1.0.0 depends on dep-b@1.0.0
    let _a = mock_npm_package_with_deps(&mut server, "dep-a", "1.0.0", &[("dep-b", "1.0.0")]);
    // dep-b@1.0.0 depends on dep-c@1.0.0
    let _b = mock_npm_package_with_deps(&mut server, "dep-b", "1.0.0", &[("dep-c", "1.0.0")]);
    // dep-c@1.0.0 has no extra deps (but mock_npm_package doesn't add any)
    let _c = mock_npm_package(&mut server, "dep-c", "1.0.0");

    let tmp = tempfile::tempdir().expect("tempdir");
    let project_dir = tmp.path().join("project");
    copy_dir(&fixture_dir, &project_dir);

    let store_home = tempfile::tempdir().expect("store home");

    let bin = ara_binary();
    let output = Command::new(&bin)
        .args(["install", "--non-interactive"])
        .current_dir(&project_dir)
        .env("HOME", store_home.path())
        .env("ARA_NPM_REGISTRY", &registry_url)
        .output()
        .expect("ara install");

    assert!(
        output.status.success(),
        "install failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let lockfile_path = project_dir.join("ara.lock");
    assert!(lockfile_path.exists(), "ara.lock not created");

    let lock_content = std::fs::read_to_string(&lockfile_path).unwrap();
    let lf = ara_lockfile::parser::parse(&lock_content).unwrap();

    let pkg_names: Vec<&str> = lf.packages.iter().map(|p| p.name.as_str()).collect();
    assert!(pkg_names.contains(&"dep-a"), "dep-a missing from lockfile");
    assert!(pkg_names.contains(&"dep-b"), "dep-b missing from lockfile");
    assert!(pkg_names.contains(&"dep-c"), "dep-c missing from lockfile");
}

#[test]
fn test_install_bin_links() {
    let r = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let fixture_dir = r.join("valid/14-bin-links");

    let mut server = mockito::Server::new();
    let registry_url = server.url();
    server.mock("GET", "/favicon.ico").with_status(404).create();

    // cli-tool@1.0.0 — tarball contains package.json with "bin" field + bin/cli.js
    let bin_js: &[u8] = b"#!/usr/bin/env node\nconsole.log('hello');\n";
    let pkg_json = serde_json::json!({
        "name": "cli-tool",
        "version": "1.0.0",
        "bin": { "cli-tool": "./bin/cli.js" }
    });
    let tarball = make_tarball_with_files(&[
        ("package.json", pkg_json.to_string().as_bytes()),
        ("bin/cli.js", bin_js),
    ]);

    let versions_body = serde_json::json!({
        "name": "cli-tool",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "cli-tool",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{}/cli-tool/-/cli-tool-1.0.0.tgz", registry_url)
                }
            }
        }
    });
    let _m1 = server
        .mock("GET", "/cli-tool")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(versions_body.to_string())
        .create();
    let _m2 = server
        .mock("GET", "/cli-tool/-/cli-tool-1.0.0.tgz")
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .with_body(tarball)
        .create();

    let tmp = tempfile::tempdir().expect("tempdir");
    let project_dir = tmp.path().join("project");
    copy_dir(&fixture_dir, &project_dir);
    let store_home = tempfile::tempdir().expect("store home");

    let bin = ara_binary();
    let output = Command::new(&bin)
        .args(["install", "--non-interactive"])
        .current_dir(&project_dir)
        .env("HOME", store_home.path())
        .env("ARA_NPM_REGISTRY", &registry_url)
        .output()
        .expect("ara install");

    assert!(
        output.status.success(),
        "install failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bin_link = project_dir.join("node_modules/.bin/cli-tool");
    assert!(bin_link.exists(), "bin link not found at {:?}", bin_link);

    // Verify it's a symlink (or at least a regular file)
    let meta = std::fs::metadata(&bin_link).expect("failed to read bin link metadata");
    assert!(
        meta.is_symlink() || meta.is_file(),
        "expected bin link to be a symlink or file"
    );
}

#[test]
fn test_install_security_warning() {
    let r = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let fixture_dir = r.join("valid/15-security-warning");

    let mut server = mockito::Server::new();
    let registry_url = server.url();
    server.mock("GET", "/favicon.ico").with_status(404).create();

    // mal-pkg@1.0.0 — tarball contains code with eval() to trigger scanner
    let tarball = make_tarball_with_files(&[("index.js", b"var x = 'user input';\neval(x);\n")]);

    let versions_body = serde_json::json!({
        "name": "mal-pkg",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "mal-pkg",
                "version": "1.0.0",
                "dist": {
                    "tarball": format!("{}/mal-pkg/-/mal-pkg-1.0.0.tgz", registry_url)
                }
            }
        }
    });
    let _m1 = server
        .mock("GET", "/mal-pkg")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(versions_body.to_string())
        .create();
    let _m2 = server
        .mock("GET", "/mal-pkg/-/mal-pkg-1.0.0.tgz")
        .with_status(200)
        .with_header("content-type", "application/octet-stream")
        .with_body(tarball)
        .create();

    let tmp = tempfile::tempdir().expect("tempdir");
    let project_dir = tmp.path().join("project");
    copy_dir(&fixture_dir, &project_dir);
    let store_home = tempfile::tempdir().expect("store home");

    let bin = ara_binary();
    let output = Command::new(&bin)
        .args(["install", "--non-interactive"])
        .current_dir(&project_dir)
        .env("HOME", store_home.path())
        .env("ARA_NPM_REGISTRY", &registry_url)
        .output()
        .expect("ara install");

    // In --non-interactive mode, install succeeds but prints a warning
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "install should succeed in non-interactive mode\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let all_output = format!("{stdout}\n{stderr}");
    assert!(
        all_output.contains("finding") || all_output.contains("eval"),
        "expected security warning in output:\n{}",
        all_output
    );
}

// ---------------------------------------------------------------------------
// Catalog fixture tests
// ---------------------------------------------------------------------------

#[test]
fn test_catalog_install_member_deps() {
    let _r = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");

    let mut server = mockito::Server::new();
    let registry_url = server.url();
    server.mock("GET", "/favicon.ico").with_status(404).create();

    let _react = mock_npm_package(&mut server, "react", "^19.0.0");

    let tmp = tempfile::tempdir().expect("tempdir");
    let project_dir = tmp.path().join("project");
    let packages_dir = project_dir.join("packages");
    let member_dir = packages_dir.join("web");
    std::fs::create_dir_all(&member_dir).expect("create member dir");

    // Root with catalog + workspace
    let root_manifest = r#"[project]
name = "monorepo"
version = "1.0.0"

[workspace]
members = ["packages/*"]

[workspace.catalog]
react = "^19.0.0"
"#;
    std::fs::write(project_dir.join("ara.toml"), root_manifest).unwrap();

    // Member using catalog:
    let member_json = r#"{
        "name": "web",
        "version": "0.1.0",
        "dependencies": {
            "react": "catalog:"
        }
    }"#;
    std::fs::write(member_dir.join("package.json"), member_json).unwrap();

    let store_home = tempfile::tempdir().expect("store home");

    let bin = ara_binary();
    let output = Command::new(&bin)
        .args(["install", "--non-interactive"])
        .current_dir(&project_dir)
        .env("HOME", store_home.path())
        .env("ARA_NPM_REGISTRY", &registry_url)
        .output()
        .expect("ara install");

    assert!(
        output.status.success(),
        "install failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify lockfile has react with resolved version
    let lockfile_path = project_dir.join("ara.lock");
    assert!(lockfile_path.exists(), "ara.lock not created");
    let lock_content = std::fs::read_to_string(&lockfile_path).unwrap();
    let lf = ara_lockfile::parser::parse(&lock_content).unwrap();

    let react_pkg = lf.packages.iter().find(|p| p.name == "react");
    assert!(react_pkg.is_some(), "react missing from lockfile");
    assert_eq!(react_pkg.unwrap().version, "19.0.0");

    // Verify lockfile contains workspace catalog
    assert!(
        lf.workspace.is_some(),
        "lockfile should contain workspace catalog"
    );
    let ws = lf.workspace.as_ref().unwrap();
    assert!(ws.catalog.is_some(), "workspace should have catalog");
    let cat = ws.catalog.as_ref().unwrap();
    assert_eq!(cat.get("react").unwrap(), "^19.0.0");
}

#[test]
fn test_catalog_install_root_deps() {
    let mut server = mockito::Server::new();
    let registry_url = server.url();
    server.mock("GET", "/favicon.ico").with_status(404).create();

    let _react = mock_npm_package(&mut server, "react", "19.0.0");

    let tmp = tempfile::tempdir().expect("tempdir");
    let project_dir = tmp.path().join("project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");

    let root_manifest = r#"[project]
name = "my-app"
version = "1.0.0"

[workspace.catalog]
react = "^19.0.0"

[deps]
react = { source = "npm", version = "catalog:" }
"#;
    std::fs::write(project_dir.join("ara.toml"), root_manifest).unwrap();

    let store_home = tempfile::tempdir().expect("store home");

    let bin = ara_binary();
    let output = Command::new(&bin)
        .args(["install", "--non-interactive"])
        .current_dir(&project_dir)
        .env("HOME", store_home.path())
        .env("ARA_NPM_REGISTRY", &registry_url)
        .output()
        .expect("ara install");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "install failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let lockfile_path = project_dir.join("ara.lock");
    assert!(lockfile_path.exists(), "ara.lock not created");
    let lock_content = std::fs::read_to_string(&lockfile_path).unwrap();
    let lf = ara_lockfile::parser::parse(&lock_content).unwrap();

    let react_pkg = lf.packages.iter().find(|p| p.name == "react");
    assert!(react_pkg.is_some(), "react missing from lockfile");
    assert_eq!(react_pkg.unwrap().version, "19.0.0");
}

#[test]
fn test_catalog_install_override_warning() {
    let mut server = mockito::Server::new();
    let registry_url = server.url();
    server.mock("GET", "/favicon.ico").with_status(404).create();

    let _react = mock_npm_package(&mut server, "react", "18.3.0");

    let tmp = tempfile::tempdir().expect("tempdir");
    let project_dir = tmp.path().join("project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");

    let root_manifest = r#"[project]
name = "my-app"
version = "1.0.0"

[workspace.catalog]
react = "^19.0.0"

[deps]
react = { source = "npm", version = "^18.3.0" }
"#;
    std::fs::write(project_dir.join("ara.toml"), root_manifest).unwrap();

    let store_home = tempfile::tempdir().expect("store home");

    let bin = ara_binary();
    let output = Command::new(&bin)
        .args(["install", "--non-interactive"])
        .current_dir(&project_dir)
        .env("HOME", store_home.path())
        .env("ARA_NPM_REGISTRY", &registry_url)
        .output()
        .expect("ara install");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "install should succeed despite override\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let all_output = format!("{stdout}\n{stderr}");
    assert!(
        all_output.to_lowercase().contains("warning")
            && all_output.to_lowercase().contains("override"),
        "expected override warning in output:\n{all_output}"
    );

    let lockfile_path = project_dir.join("ara.lock");
    assert!(lockfile_path.exists(), "ara.lock not created");
    let lock_content = std::fs::read_to_string(&lockfile_path).unwrap();
    let lf = ara_lockfile::parser::parse(&lock_content).unwrap();

    let react_pkg = lf.packages.iter().find(|p| p.name == "react").unwrap();
    assert_eq!(react_pkg.version, "18.3.0");
}

// ---------------------------------------------------------------------------
// Catalog CLI command tests
// ---------------------------------------------------------------------------

#[test]
fn test_catalog_cli_add_and_list() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project_dir = tmp.path().join("project");
    std::fs::create_dir_all(&project_dir).expect("create dir");

    let bin = ara_binary();

    // ara catalog add react ^19.0.0
    let add_output = Command::new(&bin)
        .args(["catalog", "add", "react", "^19.0.0"])
        .current_dir(&project_dir)
        .output()
        .expect("ara catalog add");

    assert!(
        add_output.status.success(),
        "catalog add failed:\n{}",
        String::from_utf8_lossy(&add_output.stderr)
    );

    // Verify ara.toml was created
    let manifest_content =
        std::fs::read_to_string(project_dir.join("ara.toml")).expect("read ara.toml");
    assert!(manifest_content.contains("react = \"^19.0.0\""));

    // ara catalog list
    let list_output = Command::new(&bin)
        .args(["catalog", "list"])
        .current_dir(&project_dir)
        .output()
        .expect("ara catalog list");

    assert!(
        list_output.status.success(),
        "catalog list failed:\n{}",
        String::from_utf8_lossy(&list_output.stderr)
    );

    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(list_stdout.contains("react"));
    assert!(list_stdout.contains("^19.0.0"));
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
