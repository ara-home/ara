use anyhow::{Context, Result};

use crate::analysis::analyzer;

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
