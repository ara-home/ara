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
    #[must_use]
    pub fn new() -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("ara-package-manager/0.1.0")
            .build()
            .expect("failed to build HTTP client");
        Self { client }
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

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}
