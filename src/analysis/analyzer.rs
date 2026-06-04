//! Static security analysis for npm packages. Scans source files for
//! suspicious patterns (eval, `child_process`, credential access, etc.)
//! and reports findings with severity levels.

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;

use crate::types::{AnalysisResult, Finding, RiskLevel};

use super::patterns::{all_patterns, install_scripts_pattern, Pattern};
use super::scanner::scan_package;

static COMPILED: LazyLock<Vec<(&'static Pattern, Regex)>> = LazyLock::new(|| {
    all_patterns()
        .iter()
        .filter_map(|p| {
            if p.regex.is_empty() {
                return None;
            }
            match Regex::new(p.regex) {
                Ok(re) => Some((p, re)),
                Err(e) => {
                    eprintln!("warning: failed to compile regex for `{}`: {e}", p.id);
                    None
                }
            }
        })
        .collect()
});

fn glob_matches(glob: &str, filename: &str) -> bool {
    if glob == filename {
        return true;
    }
    // Handle *.{ext1,ext2,...} pattern
    if let Some(extensions) = glob.strip_prefix("*.") {
        if extensions.starts_with('{') && extensions.ends_with('}') {
            let inner = &extensions[1..extensions.len() - 1];
            return inner.split(',').any(|ext| {
                let ext = ext.trim();
                filename.ends_with(&format!(".{ext}"))
            });
        }
        // Simple *.ext pattern
        return filename.ends_with(glob.strip_prefix('*').unwrap_or(glob));
    }
    false
}

fn check_install_scripts(package_json_content: &str, pattern: &Pattern) -> Vec<Finding> {
    let install_keys = ["preinstall", "install", "postinstall"];
    let has_script = install_keys.iter().any(|key| {
        let search = format!("\"{key}\"");
        package_json_content.contains(&search)
    });

    if has_script {
        vec![Finding {
            pattern: pattern.id.to_string(),
            severity: pattern.severity,
            location: Some("package.json".to_string()),
            description: pattern.description.to_string(),
        }]
    } else {
        Vec::new()
    }
}

fn make_finding(pattern: &Pattern, location: String) -> Finding {
    Finding {
        pattern: pattern.id.to_string(),
        severity: pattern.severity,
        location: Some(location),
        description: pattern.description.to_string(),
    }
}

fn aggregate_risk_level<'a>(findings: impl Iterator<Item = &'a Finding>) -> RiskLevel {
    findings.map(|f| f.severity).max().unwrap_or(RiskLevel::Low)
}

fn find_line_number(content: &str, byte_offset: usize) -> usize {
    content[..byte_offset].matches('\n').count() + 1
}

pub fn analyze_package(package_path: &Path) -> Result<AnalysisResult> {
    let files = scan_package(package_path)?;

    let compiled = &*COMPILED;
    let install_script_pat = install_scripts_pattern();

    let mut findings: Vec<Finding> = Vec::new();
    let mut seen: HashSet<(usize, &str, usize)> = HashSet::new();

    for (file_idx, file) in files.iter().enumerate() {
        let filename = file.path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if filename == "package.json" {
            findings.extend(check_install_scripts(&file.content, &install_script_pat));
            continue;
        }

        // run code patterns using regex find_iter for precise locations
        for (pattern, re) in compiled {
            if !glob_matches(pattern.file_glob, filename) {
                continue;
            }

            for m in re.find_iter(&file.content) {
                let dedup_key = (file_idx, pattern.id, m.start());
                if seen.insert(dedup_key) {
                    let line_num = find_line_number(&file.content, m.start());
                    let location = format!("{}:{}", file.path.display(), line_num);
                    findings.push(make_finding(pattern, location));
                }
            }
        }
    }

    let risk_level = aggregate_risk_level(findings.iter());

    Ok(AnalysisResult {
        risk_level,
        findings,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use std::fs;
    use std::io::Write;

    fn create_temp_package(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        for (rel_path, content) in files {
            let full = dir.path().join(rel_path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            let mut f = fs::File::create(&full).unwrap();
            f.write_all(content.as_bytes()).unwrap();
        }
        dir
    }

    #[test]
    fn test_clean_package() {
        let dir = create_temp_package(&[("index.js", "console.log('hello world');\n")]);
        let result = analyze_package(dir.path()).unwrap();
        assert_eq!(result.risk_level, RiskLevel::Low);
        assert!(result.findings.is_empty());
    }

    #[test]
    fn test_eval_detected() {
        let dir = create_temp_package(&[("index.js", "eval(user_input);\n")]);
        let result = analyze_package(dir.path()).unwrap();
        assert_eq!(result.risk_level, RiskLevel::Critical);
        assert!(result.findings.iter().any(|f| f.pattern == "eval-usage"));
    }

    #[test]
    fn test_multiple_findings() {
        let dir = create_temp_package(&[("index.js", "eval(code);\nexec(cmd);\n")]);
        let result = analyze_package(dir.path()).unwrap();
        assert_eq!(result.risk_level, RiskLevel::Critical);
        assert!(result.findings.iter().any(|f| f.pattern == "eval-usage"));
        assert!(result
            .findings
            .iter()
            .any(|f| f.pattern == "child-process-exec"));
    }

    #[test]
    fn test_ts_file_also_scanned() {
        let dir = create_temp_package(&[("app.ts", "eval(x);\n")]);
        let result = analyze_package(dir.path()).unwrap();
        assert!(result.findings.iter().any(|f| f.pattern == "eval-usage"));
    }

    #[test]
    fn test_tsx_file_scanned() {
        let dir = create_temp_package(&[("component.tsx", "new Function('return x');\n")]);
        let result = analyze_package(dir.path()).unwrap();
        assert!(result.findings.iter().any(|f| f.pattern == "new-function"));
    }

    #[test]
    fn test_deduplication_same_line() {
        let dir = create_temp_package(&[("index.js", "eval(code); // intentional eval usage\n")]);
        let result = analyze_package(dir.path()).unwrap();
        let eval_count = result
            .findings
            .iter()
            .filter(|f| f.pattern == "eval-usage")
            .count();
        assert_eq!(eval_count, 1);
    }

    #[test]
    fn test_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let result = analyze_package(dir.path()).unwrap();
        assert_eq!(result.risk_level, RiskLevel::Low);
        assert!(result.findings.is_empty());
    }

    #[test]
    fn test_package_json_install_scripts() {
        let dir = create_temp_package(&[(
            "package.json",
            "{\n  \"scripts\": {\n    \"install\": \"make evil\"\n  }\n}",
        )]);
        let result = analyze_package(dir.path()).unwrap();
        assert!(result
            .findings
            .iter()
            .any(|f| f.pattern == "install-scripts"));
    }

    #[test]
    fn test_glob_matches_source() {
        assert!(glob_matches("*.{js,ts,jsx,tsx}", "index.js"));
        assert!(glob_matches("*.{js,ts,jsx,tsx}", "lib.ts"));
        assert!(glob_matches("*.{js,ts,jsx,tsx}", "component.tsx"));
        assert!(glob_matches("*.{js,ts,jsx,tsx}", "styles.jsx"));
        assert!(!glob_matches("*.{js,ts,jsx,tsx}", "package.json"));
        assert!(!glob_matches("*.{js,ts,jsx,tsx}", "readme.md"));
        assert!(glob_matches("package.json", "package.json"));
        assert!(!glob_matches("package.json", "index.js"));
    }

    #[test]
    fn test_nonexistent_package() {
        let result = analyze_package(Path::new("/tmp/nonexistent_ara_test_xyz"));
        assert!(result.is_err());
    }

    #[test]
    fn test_risk_level_aggregation() {
        let f1 = Finding {
            pattern: "math-random".into(),
            severity: RiskLevel::Low,
            location: Some("a.js:1".into()),
            description: "test".into(),
        };
        let f2 = Finding {
            pattern: "eval-usage".into(),
            severity: RiskLevel::Critical,
            location: Some("b.js:1".into()),
            description: "test".into(),
        };
        let level = aggregate_risk_level([f1, f2].iter());
        assert_eq!(level, RiskLevel::Critical);
    }

    #[test]
    fn test_mjs_cjs_supported() {
        let dir =
            create_temp_package(&[("module.mjs", "eval(x);\n"), ("module.cjs", "eval(y);\n")]);
        let result = analyze_package(dir.path()).unwrap();
        assert!(result.findings.iter().any(|f| f.pattern == "eval-usage"));
    }

    #[test]
    fn test_mts_cts_supported() {
        let dir =
            create_temp_package(&[("module.mts", "eval(a);\n"), ("module.cts", "eval(b);\n")]);
        let result = analyze_package(dir.path()).unwrap();
        assert!(result.findings.iter().any(|f| f.pattern == "eval-usage"));
    }

    #[cfg(feature = "nightly-bench")]
    fn create_bench_analysis_dir(n: usize) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..n {
            let name = format!("src/file_{i:06}.js");
            let content = format!("const x = {i};\neval(x);\n");
            let full = dir.path().join(&name);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&full, content).unwrap();
        }
        dir
    }

    #[cfg(feature = "nightly-bench")]
    #[bench]
    fn bench_analyze_100(b: &mut test::Bencher) {
        let dir = create_bench_analysis_dir(100);
        b.iter(|| analyze_package(test::black_box(dir.path())).unwrap());
    }

    #[cfg(feature = "nightly-bench")]
    #[bench]
    fn bench_analyze_1000(b: &mut test::Bencher) {
        let dir = create_bench_analysis_dir(1000);
        b.iter(|| analyze_package(test::black_box(dir.path())).unwrap());
    }
}
