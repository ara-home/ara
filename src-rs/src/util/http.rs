use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("non-success status: {0}")]
    StatusNotOk(reqwest::StatusCode),
}

pub struct HttpClient {
    client: reqwest::blocking::Client,
}

impl HttpClient {
    pub fn new() -> Result<Self, HttpError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("ara-package-manager/0.1.0")
            .build()?;
        Ok(Self { client })
    }

    pub fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        let resp = self.client.get(url).send()?;
        let status = resp.status();
        if !status.is_success() {
            return Err(HttpError::StatusNotOk(status));
        }
        let body = resp.bytes()?.to_vec();
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn test_http_client_new_ok() {
        let client = HttpClient::new().unwrap();
        // Just verify it doesn't crash — timeout isn't publicly accessible
        let _ = client;
    }

    #[test]
    fn test_http_client_get_200() {
        let mut server = mockito::Server::new();
        let url = server.url();
        let mock = server
            .mock("GET", "/data")
            .with_status(200)
            .with_body("hello world")
            .create();

        let client = HttpClient::new().unwrap();
        let body = client.get(&format!("{url}/data")).unwrap();
        assert_eq!(body, b"hello world");
        mock.assert();
    }

    #[test]
    fn test_http_client_get_404() {
        let mut server = mockito::Server::new();
        let url = server.url();
        let mock = server.mock("GET", "/missing").with_status(404).create();

        let client = HttpClient::new().unwrap();
        let err = client.get(&format!("{url}/missing")).unwrap_err();
        assert!(matches!(err, HttpError::StatusNotOk(s) if s == 404));
        mock.assert();
    }
}
