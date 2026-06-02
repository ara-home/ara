use serde::{Deserialize, Serialize};

/// Supported IPC methods.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    Analyze,
    Scan,
    Verify,
    Audit,
    Shutdown,
}

/// JSON-RPC request from the host (ara).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: Method,
    pub params: serde_json::Value,
}

/// JSON-RPC response sent back to the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    #[must_use]
    pub fn ok(id: u64, result: serde_json::Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    #[must_use]
    pub fn err(id: u64, code: i32, message: impl Into<String>) -> Self {
        Self {
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

/// Error payload returned on failure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

/// Risk classification for a package or finding.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// A single security finding discovered during analysis.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub pattern: String,
    pub severity: RiskLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    pub description: String,
}

/// Complete result returned by the `analyze` method.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub risk_level: RiskLevel,
    pub findings: Vec<Finding>,
}

/// Result returned by the `verify` method.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifyResult {
    pub valid: bool,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_ok() {
        let resp = Response::ok(1, serde_json::json!({"status": "ok"}));
        assert_eq!(resp.id, 1);
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap(), serde_json::json!({"status": "ok"}));
    }

    #[test]
    fn test_response_err() {
        let resp = Response::err(2, -1, "something went wrong");
        assert_eq!(resp.id, 2);
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -1);
        assert_eq!(err.message, "something went wrong");
    }

    #[test]
    fn test_serialize_parse_request() {
        let req = Request {
            id: 42,
            method: Method::Analyze,
            params: serde_json::json!({"path": "/tmp/pkg"}),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 42);
        assert_eq!(parsed.method, Method::Analyze);
    }

    #[test]
    fn test_risk_level_order() {
        assert!(RiskLevel::Low < RiskLevel::Medium);
        assert!(RiskLevel::Medium < RiskLevel::High);
        assert!(RiskLevel::High < RiskLevel::Critical);
    }

    #[test]
    fn test_finding_creation() {
        let f = Finding {
            pattern: "eval-usage".into(),
            severity: RiskLevel::Medium,
            location: Some("lib/utils.js:15".into()),
            description: "Dynamic code evaluation detected".into(),
        };
        assert_eq!(f.severity, RiskLevel::Medium);
        assert!(f.location.is_some());
    }

    #[test]
    fn test_verify_result() {
        let r = VerifyResult {
            valid: true,
            message: "signature verified".into(),
        };
        assert!(r.valid);
    }

    #[test]
    fn test_analysis_result_serialize() {
        let result = AnalysisResult {
            risk_level: RiskLevel::High,
            findings: vec![Finding {
                pattern: "credential-access".into(),
                severity: RiskLevel::High,
                location: Some("src/main.js:42".into()),
                description: "Access to process.env.GITHUB_TOKEN".into(),
            }],
        };
        let json = serde_json::to_string_pretty(&result).unwrap();
        assert!(json.contains("high"));
        assert!(json.contains("credential-access"));
    }
}
