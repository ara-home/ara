use anyhow::{Context, Result};

use crate::analysis::analyzer;

pub(crate) fn cmd_analyze(path: &str) -> Result<()> {
    let abs_path = std::fs::canonicalize(path).context("invalid path")?;
    println!("Analyzing {}...\n", abs_path.display());

    match analyzer::analyze_package(&abs_path) {
        Ok(result) => {
            if result.findings.is_empty() {
                println!("  No suspicious patterns detected.");
            } else {
                super::prompt::print_findings(&result.findings, result.risk_level);
            }
        }
        Err(e) => {
            eprintln!("  Analysis failed: {e}");
        }
    }
    Ok(())
}

pub(crate) fn cmd_audit(path: &str) -> Result<()> {
    let abs_path = std::fs::canonicalize(path).context("invalid path")?;
    println!("Auditing {}...\n", abs_path.display());

    match analyzer::analyze_package(&abs_path) {
        Ok(result) => {
            let summary = if result.findings.is_empty() {
                "No issues found.".to_string()
            } else {
                format!("Found {} potential issue(s).", result.findings.len())
            };

            if result.findings.is_empty() {
                println!("  No suspicious patterns detected.");
            } else {
                super::prompt::print_findings(&result.findings, result.risk_level);
            }
            println!("\n  Summary: {summary}");
        }
        Err(e) => {
            eprintln!("  Audit failed: {e}");
        }
    }
    Ok(())
}
