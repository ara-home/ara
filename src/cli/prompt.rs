use std::io::{self, IsTerminal, Write};

use crate::types::RiskLevel;

fn severity_color(severity: &str) -> &'static str {
    match severity {
        "critical" => "\x1b[31m",
        "high" => "\x1b[33m",
        "medium" => "\x1b[36m",
        "low" => "\x1b[32m",
        _ => "\x1b[0m",
    }
}

fn severity_label(severity: &str) -> &'static str {
    match severity {
        "critical" => "CRITICAL",
        "high" => "HIGH",
        "medium" => "MEDIUM",
        "low" => "LOW",
        _ => "UNKNOWN",
    }
}

pub(crate) fn print_findings(findings: &[crate::types::Finding], risk_level: RiskLevel) {
    let reset = "\x1b[0m";
    for f in findings {
        let color = severity_color(&f.severity.to_string());
        let label = severity_label(&f.severity.to_string());
        let location = f.location.as_deref().unwrap_or("-");
        println!(
            "  {color}{label:>8}{reset}  {:<20}  {:<25}  {}",
            f.pattern, location, f.description
        );
    }
    println!(
        "\n  Risk level: {}{}{reset}",
        severity_color(&risk_level.to_string()),
        risk_level
    );
}

pub(crate) enum AllowDecision {
    Yes,
    No,
    Sandbox,
}

pub(crate) fn prompt_allow_package(
    name: &str,
    version: &str,
    findings: &[crate::types::Finding],
) -> AllowDecision {
    if !io::stdin().is_terminal() {
        return AllowDecision::Yes;
    }

    println!("\n  ⚠  {name}@{version} wants to:");
    let reset = "\x1b[0m";
    for f in findings {
        let color = severity_color(&f.severity.to_string());
        let label = severity_label(&f.severity.to_string());
        println!(
            "  {color}· {:<30} [{label}]{reset}  {}",
            f.description, f.pattern
        );
    }

    loop {
        print!("\n  Allow? (yes / no / sandbox / inspect) ");
        let _ = io::stdout().flush();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() || input.trim().is_empty() {
            return AllowDecision::Yes;
        }

        match input.trim().to_lowercase().as_str() {
            "yes" | "y" => return AllowDecision::Yes,
            "no" | "n" => return AllowDecision::No,
            "sandbox" | "s" => return AllowDecision::Sandbox,
            "inspect" | "i" => {
                let risk = findings
                    .iter()
                    .map(|f| f.severity)
                    .max()
                    .unwrap_or(RiskLevel::Low);
                print_findings(findings, risk);
            }
            _ => {
                println!("  Invalid option. Please type yes, no, sandbox, or inspect.");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_severity_color() {
        assert_eq!(severity_color("critical"), "\x1b[31m");
        assert_eq!(severity_color("high"), "\x1b[33m");
        assert_eq!(severity_color("medium"), "\x1b[36m");
        assert_eq!(severity_color("low"), "\x1b[32m");
        assert_eq!(severity_color("unknown"), "\x1b[0m");
    }

    #[test]
    fn test_severity_label() {
        assert_eq!(severity_label("critical"), "CRITICAL");
        assert_eq!(severity_label("high"), "HIGH");
        assert_eq!(severity_label("medium"), "MEDIUM");
        assert_eq!(severity_label("low"), "LOW");
        assert_eq!(severity_label("unknown"), "UNKNOWN");
    }

    #[test]
    fn test_print_findings_does_not_crash() {
        let findings = vec![crate::types::Finding {
            pattern: "eval-usage".into(),
            severity: crate::types::RiskLevel::Critical,
            location: Some("index.js:1".into()),
            description: "eval detected".into(),
        }];
        print_findings(&findings, crate::types::RiskLevel::Critical);
    }
}
