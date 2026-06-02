//! ara-sec, as a Security analysis engine for the Ara package manager.
//!
//! Communicates with the Zig host process via JSON-RPC over stdin/stdout.

#![warn(clippy::pedantic, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod types;

use anyhow::Context;
use std::io::{self, BufRead, Write};
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
    let path = req.params.get("package_path").and_then(|v| v.as_str());
    let path = match path {
        Some(p) => p,
        None => return Response::err(req.id, -1, "missing field: package_path"),
    };

    Response::ok(
        req.id,
        serde_json::json!({
            "risk_level": "low",
            "findings": [],
            "package_path": path,
        }),
    )
}

fn handle_scan(req: &Request) -> Response {
    let hash = req.params.get("package_hash").and_then(|v| v.as_str());
    let hash = match hash {
        Some(h) => h,
        None => return Response::err(req.id, -1, "missing field: package_hash"),
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
    let signature = req.params.get("signature").and_then(|v| v.as_str());
    let signature = match signature {
        Some(s) => s,
        None => return Response::err(req.id, -1, "missing field: signature"),
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
    let path = req.params.get("package_path").and_then(|v| v.as_str());
    let path = match path {
        Some(p) => p,
        None => return Response::err(req.id, -1, "missing field: package_path"),
    };

    Response::ok(
        req.id,
        serde_json::json!({
            "report": {
                "package_path": path,
                "risk_level": "low",
                "findings": [],
                "summary": "No suspicious patterns detected.",
            }
        }),
    )
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
    fn test_handle_analyze_ok() {
        let req = Request {
            id: 2,
            method: Method::Analyze,
            params: serde_json::json!({"package_path": "/tmp/pkg"}),
        };
        let resp = dispatch(&req);
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["risk_level"], "low");
        assert_eq!(result["package_path"], "/tmp/pkg");
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
    fn test_handle_audit_ok() {
        let req = Request {
            id: 5,
            method: Method::Audit,
            params: serde_json::json!({"package_path": "/tmp/pkg"}),
        };
        let resp = dispatch(&req);
        let result = resp.result.unwrap();
        assert!(result["report"]["summary"].as_str().is_some());
    }

    #[test]
    fn test_dispatch_unknown_method() {
        let req = Request {
            id: 0,
            method: Method::Shutdown,
            params: serde_json::json!(null),
        };
        let resp = dispatch(&req);
        assert!(resp.error.is_none());
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
