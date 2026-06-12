use anyhow::{Context, Result};

use ara_analysis::analyzer;

fn run_analysis(path: &str, label: &str, show_summary: bool) -> Result<()> {
    let abs_path = std::fs::canonicalize(path).context("invalid path")?;
    println!("{label} {}...\n", abs_path.display());

    match analyzer::analyze_package(&abs_path) {
        Ok(result) => {
            if result.findings.is_empty() {
                println!("  No suspicious patterns detected.");
            } else {
                super::prompt::print_findings(&result.findings, result.risk_level);
            }
            if show_summary {
                let summary = if result.findings.is_empty() {
                    "No issues found."
                } else {
                    "Found potential issue(s)."
                };
                println!("\n  Summary: {summary}");
            }
        }
        Err(e) => {
            eprintln!("  {label} failed: {e}");
        }
    }
    Ok(())
}

pub(crate) fn cmd_analyze(path: &str) -> Result<()> {
    run_analysis(path, "Analyzing", false)
}

pub(crate) fn cmd_audit(path: &str) -> Result<()> {
    run_analysis(path, "Auditing", true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_analyze_invalid_path() {
        let res = cmd_analyze("/path/that/definitely/does/not/exist/12345");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("invalid path"));
    }

    #[test]
    fn test_cmd_audit_invalid_path() {
        let res = cmd_audit("/path/that/definitely/does/not/exist/12345");
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("invalid path"));
    }
}
