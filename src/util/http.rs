use std::sync::OnceLock;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT};

const MAX_RETRIES: u32 = 3;
const BASE_DELAY_MS: u64 = 100;

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("non-success status: {0}")]
    StatusNotOk(reqwest::StatusCode),
    #[error("max retries exceeded")]
    MaxRetries,
}

fn shared_client() -> Result<reqwest::Client, HttpError> {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

    if let Some(c) = CLIENT.get() {
        return Ok(c.clone());
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static(
            "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*",
        ),
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent("ara-package-manager/0.1.0")
        .default_headers(headers)
        .pool_max_idle_per_host(512)
        .tcp_keepalive(Duration::from_secs(60))
        .build()?;

    let _ = CLIENT.set(client.clone());
    Ok(client)
}

#[derive(Clone)]
pub struct HttpClient {
    client: reqwest::Client,
}

impl HttpClient {
    pub fn new() -> Result<Self, HttpError> {
        Ok(Self {
            client: shared_client()?,
        })
    }

    fn should_retry(status: Option<reqwest::StatusCode>) -> bool {
        match status {
            None => true,
            Some(s) => s.is_server_error(),
        }
    }

    pub async fn get(&self, url: &str) -> Result<Vec<u8>, HttpError> {
        let mut last_error = None;

        for attempt in 0..MAX_RETRIES {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(BASE_DELAY_MS * (1 << attempt))).await;
            }

            match self.client.get(url).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        match resp.bytes().await {
                            Ok(bytes) => return Ok(bytes.to_vec()),
                            Err(e) => {
                                last_error = Some(HttpError::Request(e));
                                continue;
                            }
                        }
                    }
                    if !Self::should_retry(Some(status)) {
                        return Err(HttpError::StatusNotOk(status));
                    }
                    last_error = Some(HttpError::StatusNotOk(status));
                }
                Err(e) => {
                    if !Self::should_retry(None) {
                        return Err(HttpError::Request(e));
                    }
                    last_error = Some(HttpError::Request(e));
                }
            }
        }

        Err(last_error.unwrap_or(HttpError::MaxRetries))
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

    #[tokio::test]
    async fn test_http_client_get_200() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        let mock = server
            .mock("GET", "/data")
            .with_status(200)
            .with_body("hello world")
            .create_async()
            .await;

        let client = HttpClient::new().unwrap();
        let body = client.get(&format!("{url}/data")).await.unwrap();
        assert_eq!(body, b"hello world");
        mock.assert();
    }

    #[tokio::test]
    async fn test_http_client_get_404() {
        let mut server = mockito::Server::new_async().await;
        let url = server.url();
        let mock = server
            .mock("GET", "/missing")
            .with_status(404)
            .create_async()
            .await;

        let client = HttpClient::new().unwrap();
        let err = client.get(&format!("{url}/missing")).await.unwrap_err();
        assert!(matches!(err, HttpError::StatusNotOk(s) if s == 404));
        mock.assert();
    }
}
