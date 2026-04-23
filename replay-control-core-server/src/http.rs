//! Shared HTTP client and helpers for async network requests.
//!
//! Gated behind the `http` feature flag — only available on SSR builds.

use std::sync::OnceLock;

use replay_control_core::error::{Error, Result};

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Shared HTTP client with sensible defaults (User-Agent, timeouts, connection pooling).
pub fn shared_client() -> &'static reqwest::Client {
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("replay-control")
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .pool_max_idle_per_host(4)
            .build()
            .expect("Failed to create HTTP client")
    })
}

/// GET a URL with custom timeout and return the raw bytes.
pub async fn get_bytes_with_timeout(url: &str, timeout: std::time::Duration) -> Result<Vec<u8>> {
    let resp = shared_client()
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| Error::Other(format!("HTTP request failed for {url}: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Other(format!("HTTP error for {url}: {e}")))?;

    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| Error::Other(format!("Failed to read response body from {url}: {e}")))
}

/// GET a URL with custom timeout and parse as JSON.
pub async fn get_json_with_timeout(
    url: &str,
    timeout: std::time::Duration,
) -> Result<serde_json::Value> {
    let resp = shared_client()
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| Error::Other(format!("HTTP request failed for {url}: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Other(format!("HTTP error for {url}: {e}")))?;

    resp.json()
        .await
        .map_err(|e| Error::Other(format!("JSON parse error for {url}: {e}")))
}

/// Download a URL to a file on disk. Returns the number of bytes written.
pub async fn download_to_file(
    url: &str,
    dest: &std::path::Path,
    timeout: std::time::Duration,
) -> Result<u64> {
    let resp = shared_client()
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| Error::Other(format!("HTTP request failed for {url}: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Other(format!("HTTP error for {url}: {e}")))?;

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| Error::Other(format!("Failed to read response body from {url}: {e}")))?;

    let len = bytes.len() as u64;
    tokio::fs::write(dest, &bytes)
        .await
        .map_err(|e| Error::io(dest, e))?;

    Ok(len)
}

/// GET a URL with custom headers and parse as JSON.
pub async fn get_json_with_headers(
    url: &str,
    headers: &[(&str, &str)],
    timeout: std::time::Duration,
) -> Result<serde_json::Value> {
    let mut req = shared_client().get(url).timeout(timeout);
    for (key, value) in headers {
        req = req.header(*key, *value);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| Error::Other(format!("HTTP request failed for {url}: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Other(format!("HTTP error for {url}: {e}")))?;

    resp.json()
        .await
        .map_err(|e| Error::Other(format!("JSON parse error for {url}: {e}")))
}

/// GET a URL with custom timeout and return as text.
pub async fn get_text_with_timeout(url: &str, timeout: std::time::Duration) -> Result<String> {
    let resp = shared_client()
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| Error::Other(format!("HTTP request failed for {url}: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Other(format!("HTTP error for {url}: {e}")))?;

    resp.text()
        .await
        .map_err(|e| Error::Other(format!("Failed to read response text from {url}: {e}")))
}

/// GET a URL with custom headers and return raw text.
pub async fn get_text_with_headers(
    url: &str,
    headers: &[(&str, &str)],
    timeout: std::time::Duration,
) -> Result<String> {
    let mut req = shared_client().get(url).timeout(timeout);
    for (key, value) in headers {
        req = req.header(*key, *value);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| Error::Other(format!("HTTP request failed for {url}: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Other(format!("HTTP error for {url}: {e}")))?;

    resp.text()
        .await
        .map_err(|e| Error::Other(format!("Failed to read response text from {url}: {e}")))
}
