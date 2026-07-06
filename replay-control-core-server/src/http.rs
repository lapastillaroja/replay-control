//! Shared HTTP client and helpers for async network requests.
//!
//! Gated behind the `http` feature flag — only available on SSR builds.

use std::net::IpAddr;
use std::sync::OnceLock;

use replay_control_core::error::{Error, Result};

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static SSRF_GUARDED_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// True if `ip` is loopback, private, link-local, or otherwise not a routable
/// public address — the set a user-supplied download URL must never reach, so
/// the appliance can't be turned into a proxy into its own LAN or localhost.
fn is_private_or_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                // 100.64.0.0/10 carrier-grade NAT
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 0x40)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7 unique local
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|m| is_private_or_local_ip(IpAddr::V4(m)))
        }
    }
}

/// Reject a host string that is obviously an internal name.
fn is_local_hostname(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h == "localhost"
        || h.ends_with(".local")
        || h.ends_with(".localhost")
        || h.ends_with(".internal")
}

/// Validate that a user-supplied download URL is safe to fetch: it must be
/// http(s), not a local hostname, and must not resolve to any private/loopback
/// address. Guards against SSRF where a lower-privilege caller points a manual
/// download at the appliance's own LAN or localhost services.
pub async fn assert_safe_download_url(url: &str) -> Result<()> {
    let parsed = url::Url::parse(url).map_err(|_| Error::Other(format!("invalid URL: {url}")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(Error::Other("download URL must be http or https".into()));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| Error::Other("download URL has no host".into()))?;
    // An IP literal that's already local is rejected without a lookup.
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_or_local_ip(ip) {
            return Err(Error::Other(
                "download URL points at a private/internal address".into(),
            ));
        }
        return Ok(());
    }
    if is_local_hostname(host) {
        return Err(Error::Other("download URL host not allowed".into()));
    }
    let port = parsed.port_or_known_default().unwrap_or(80);
    let addrs: Vec<_> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| Error::Other(format!("could not resolve {host}: {e}")))?
        .collect();
    if addrs.is_empty() {
        return Err(Error::Other(format!("{host} did not resolve")));
    }
    if addrs.iter().any(|addr| is_private_or_local_ip(addr.ip())) {
        return Err(Error::Other(
            "download URL resolves to a private/internal address".into(),
        ));
    }
    Ok(())
}

/// HTTP client for user-supplied download URLs. Its redirect policy re-checks
/// each hop and refuses a redirect into a private/loopback host, so an allowed
/// host can't 302 the request onto an internal target.
fn ssrf_guarded_client() -> &'static reqwest::Client {
    SSRF_GUARDED_CLIENT.get_or_init(|| {
        let policy = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.error("too many redirects");
            }
            if let Some(host) = attempt.url().host_str()
                && (host.parse::<IpAddr>().is_ok_and(is_private_or_local_ip)
                    || is_local_hostname(host))
            {
                return attempt.error("redirect to private/internal host blocked");
            }
            attempt.follow()
        });
        reqwest::Client::builder()
            .user_agent("replay-control")
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .pool_max_idle_per_host(4)
            .redirect(policy)
            .build()
            .expect("Failed to create SSRF-guarded HTTP client")
    })
}

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
        .map_err(|e| Error::Other(format!("HTTP request failed for {url}: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        let retry_hint = retry_after
            .map(|seconds| format!("; retry_after={seconds}"))
            .unwrap_or_default();
        return Err(Error::Other(format!(
            "HTTP error for {url}: {status}{retry_hint}"
        )));
    }

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

/// Download a URL to a file on disk, streaming the body so it never lands in
/// memory whole, and aborting if it exceeds `max_bytes`. Returns the number of
/// bytes written. The cap protects the small-RAM appliance from a large (or
/// hostile, e.g. an SSRF-redirected) response filling memory or the backing
/// store; a partial file is removed on abort. For trusted, app-controlled URLs
/// (updates, metadata) — use [`download_to_file_guarded`] for user-supplied
/// URLs.
pub async fn download_to_file(
    url: &str,
    dest: &std::path::Path,
    timeout: std::time::Duration,
    max_bytes: u64,
) -> Result<u64> {
    download_to_file_with(shared_client(), url, dest, timeout, max_bytes).await
}

/// [`download_to_file`] for a **user-supplied** URL: validates the target is a
/// public http(s) host (not localhost/private, DNS-resolved) and follows
/// redirects only to public hosts, so a lower-privilege caller can't turn the
/// appliance into a proxy into its own LAN.
pub async fn download_to_file_guarded(
    url: &str,
    dest: &std::path::Path,
    timeout: std::time::Duration,
    max_bytes: u64,
) -> Result<u64> {
    assert_safe_download_url(url).await?;
    download_to_file_with(ssrf_guarded_client(), url, dest, timeout, max_bytes).await
}

async fn download_to_file_with(
    client: &reqwest::Client,
    url: &str,
    dest: &std::path::Path,
    timeout: std::time::Duration,
    max_bytes: u64,
) -> Result<u64> {
    use tokio::io::AsyncWriteExt;
    use tokio_stream::StreamExt;

    let resp = client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| Error::Other(format!("HTTP request failed for {url}: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Other(format!("HTTP error for {url}: {e}")))?;

    // Reject up front when the server advertises an oversized body.
    if let Some(len) = resp.content_length()
        && len > max_bytes
    {
        return Err(Error::Other(format!(
            "Download from {url} exceeds size limit ({len} > {max_bytes} bytes)"
        )));
    }

    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| Error::io(dest, e))?;

    // Stream in an inner block so every failure path — read error, cap
    // exceeded, write error, or a deferred flush error (realistic on NFS:
    // ENOSPC/EIO can surface only at close) — flows to the single cleanup
    // below and never leaves a truncated file on disk.
    let result = async {
        let mut written: u64 = 0;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| {
                Error::Other(format!("Failed to read response body from {url}: {e}"))
            })?;
            written += chunk.len() as u64;
            if written > max_bytes {
                return Err(Error::Other(format!(
                    "Download from {url} exceeds size limit ({max_bytes} bytes)"
                )));
            }
            file.write_all(&chunk)
                .await
                .map_err(|e| Error::io(dest, e))?;
        }
        file.flush().await.map_err(|e| Error::io(dest, e))?;
        Ok(written)
    }
    .await;

    if result.is_err() {
        drop(file);
        let _ = tokio::fs::remove_file(dest).await;
    }
    result
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_and_local_ips_are_rejected() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.5",
            "172.16.9.9",
            "169.254.1.1",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.1.1",
        ] {
            assert!(
                is_private_or_local_ip(ip.parse().unwrap()),
                "{ip} should be rejected"
            );
        }
    }

    #[test]
    fn public_ips_are_allowed() {
        for ip in ["8.8.8.8", "1.1.1.1", "93.184.216.34", "2606:2800:220:1::1"] {
            assert!(
                !is_private_or_local_ip(ip.parse().unwrap()),
                "{ip} should be allowed"
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn assert_safe_download_url_rejects_local_and_bad_schemes() {
        assert!(
            assert_safe_download_url("http://127.0.0.1/x")
                .await
                .is_err()
        );
        assert!(
            assert_safe_download_url("http://localhost/x")
                .await
                .is_err()
        );
        assert!(
            assert_safe_download_url("http://192.168.1.4/x")
                .await
                .is_err()
        );
        assert!(
            assert_safe_download_url("http://foo.local/x")
                .await
                .is_err()
        );
        assert!(
            assert_safe_download_url("file:///etc/passwd")
                .await
                .is_err()
        );
        assert!(
            assert_safe_download_url("ftp://example.com/x")
                .await
                .is_err()
        );
        assert!(assert_safe_download_url("not a url").await.is_err());
    }
}
