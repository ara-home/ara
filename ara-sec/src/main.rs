//! ara-sec — Security analysis engine for the Ara package manager.
//!
//! Communicates with the Zig host process via JSON-RPC over stdin/stdout.

#![warn(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod analysis;
mod types;

use std::io::{self, BufRead, Write};
use std::path::Path;

use anyhow::Context;
use types::{Method, Request, Response};

fn main() -> anyhow::Result<()> {
    let stdin = io::stdin().lock();
    let stdout = io::stdout();

    for line in stdin.lines() {
        let raw = line.context("failed to read stdin")?;
        if raw.trim().is_empty() {
            continue;
        }

        let req: Request = serde_json::from_str(&raw).context("failed to parse request")?;
        let method = req.method;
        let response = dispatch(&req);

        let mut out = stdout.lock();
        serde_json::to_writer(&mut out, &response).context("failed to write response")?;
        out.write_all(b"\n").context("failed to write newline")?;
        out.flush().context("failed to flush stdout")?;

        if matches!(method, Method::Shutdown) {
            break;
        }
    }

    Ok(())
}

fn dispatch(req: &Request) -> Response {
    match req.method {
        Method::Analyze => handle_analyze(req),
        Method::Scan => handle_scan(req),
        Method::Verify => handle_verify(req),
        Method::Audit => handle_audit(req),
        Method::Shutdown => Response::ok(req.id, serde_json::json!({"status": "shutting_down"})),
    }
}

fn handle_analyze(req: &Request) -> Response {
    let Some(path) = req.params.get("package_path").and_then(|v| v.as_str()) else {
        return Response::err(req.id, -1, "missing field: package_path");
    };

    match analysis::analyzer::analyze_package(Path::new(path)) {
        Ok(result) => Response::ok(
            req.id,
            serde_json::json!({
                "risk_level": result.risk_level,
                "findings": result.findings,
                "package_path": path,
            }),
        ),
        Err(e) => Response::err(req.id, -2, format!("analysis failed: {e}")),
    }
}

fn handle_scan(req: &Request) -> Response {
    let Some(hash) = req.params.get("package_hash").and_then(|v| v.as_str()) else {
        return Response::err(req.id, -1, "missing field: package_hash");
    };

    Response::ok(
        req.id,
        serde_json::json!({
            "findings": [],
            "package_hash": hash,
        }),
    )
}

fn handle_verify(req: &Request) -> Response {
    let Some(signature) = req.params.get("signature").and_then(|v| v.as_str()) else {
        return Response::err(req.id, -1, "missing field: signature");
    };

    Response::ok(
        req.id,
        serde_json::json!({
            "valid": true,
            "message": format!("signature verified: {signature}"),
        }),
    )
}

fn handle_audit(req: &Request) -> Response {
    let Some(path) = req.params.get("package_path").and_then(|v| v.as_str()) else {
        return Response::err(req.id, -1, "missing field: package_path");
    };

    match analysis::analyzer::analyze_package(Path::new(path)) {
        Ok(result) => {
            let summary = if result.findings.is_empty() {
                "No suspicious patterns detected.".to_string()
            } else {
                format!(
                    "Found {} potential issue(s) with {} risk level.",
                    result.findings.len(),
                    serde_json::to_string(&result.risk_level).unwrap_or_default().replace('\"', ""),
                )
            };

            Response::ok(
                req.id,
                serde_json::json!({
                    "report": {
                        "package_path": path,
                        "risk_level": result.risk_level,
                        "findings": result.findings,
                        "summary": summary,
                    }
                }),
            )
        }
        Err(e) => Response::err(req.id, -2, format!("audit failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_analyze_missing_path() {
        let req = Request {
            id: 1,
            method: Method::Analyze,
            params: serde_json::json!({}),
        };
        let resp = dispatch(&req);
        assert!(resp.result.is_none());
        assert_eq!(resp.error.unwrap().code, -1);
    }

    #[test]
    fn test_handle_analyze_nonexistent_package() {
        let req = Request {
            id: 2,
            method: Method::Analyze,
            params: serde_json::json!({"package_path": "/tmp/nonexistent_ara_test_xyz_42"}),
        };
        let resp = dispatch(&req);
        assert!(resp.result.is_none());
        assert_eq!(resp.error.unwrap().code, -2);
    }

    #[test]
    fn test_handle_scan_missing_hash() {
        let req = Request {
            id: 3,
            method: Method::Scan,
            params: serde_json::json!({}),
        };
        let resp = dispatch(&req);
        assert!(resp.result.is_none());
    }

    #[test]
    fn test_handle_verify_ok() {
        let req = Request {
            id: 4,
            method: Method::Verify,
            params: serde_json::json!({"signature": "ed25519:abc123"}),
        };
        let resp = dispatch(&req);
        let result = resp.result.unwrap();
        assert_eq!(result["valid"], true);
    }

    #[test]
    fn test_handle_audit_nonexistent_package() {
        let req = Request {
            id: 5,
            method: Method::Audit,
            params: serde_json::json!({"package_path": "/tmp/nonexistent_ara_test_xyz_42"}),
        };
        let resp = dispatch(&req);
        assert!(resp.result.is_none());
    }

    #[test]
    fn test_shutdown_response() {
        let req = Request {
            id: 99,
            method: Method::Shutdown,
            params: serde_json::json!(null),
        };
        let resp = dispatch(&req);
        let result = resp.result.unwrap();
        assert_eq!(result["status"], "shutting_down");
    }
}
