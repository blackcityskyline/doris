//! Retrying HTTP fetch with backoff and challenge short-circuiting ported from torio's
//! `util/net.ts`. Why this exists: retrying a *challenge* page is what turns a momentary block
//! into a 90-second stall.

use std::future::Future;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, RETRY_AFTER, SERVER};
use reqwest::{Client, RequestBuilder};

/// torio's `DEFAULT_RETRIES`: attempts *after* the first one.
pub const DEFAULT_RETRIES: u32 = 5;
/// torio's `DEFAULT_BASE_MS`: backoff doubles from here per attempt.
pub const DEFAULT_BASE_MS: u64 = 500;
/// torio's `DEFAULT_CAP_MS`: the ceiling one backoff step can reach.
pub const DEFAULT_CAP_MS: u64 = 20_000;

/// torio's `RETRY_STATUS`: only these statuses are worth another attempt.
pub const RETRY_STATUS: [u16; 7] = [408, 425, 429, 500, 502, 503, 504];

/// Which statuses are retried -- see [`RETRY_STATUS`].
pub fn is_retryable(status: u16) -> bool {
    RETRY_STATUS.contains(&status)
}

/// Knobs of one [`fetch_resilient`] call.
#[derive(Debug, Clone, Copy)]
pub struct FetchOptions {
    /// Attempts after the first one (torio: `retries`).
    pub retries: u32,
    /// Backoff base in milliseconds (torio: `baseMs`).
    pub base_ms: u64,
    /// Backoff ceiling in milliseconds (torio: `capMs`).
    pub cap_ms: u64,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            retries: DEFAULT_RETRIES,
            base_ms: DEFAULT_BASE_MS,
            cap_ms: DEFAULT_CAP_MS,
        }
    }
}

impl FetchOptions {
    /// How long to wait before attempt `attempt` + 1.
    fn delay(&self, attempt: u32, retry_after_ms: Option<u64>) -> u64 {
        backoff_delay(
            attempt,
            self.base_ms,
            self.cap_ms,
            retry_after_ms,
            rand_fraction(),
        )
    }
}

/// The shared client for HTTP sources: uniform browser-like headers, so a request missing what
/// a real browser always sends is not an easy bot-detection signal.
pub fn browser_client() -> Client {
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
    );
    headers.insert(
        ACCEPT_LANGUAGE,
        HeaderValue::from_static("ru-RU,ru;q=0.9,en-US;q=0.8,en;q=0.7"),
    );
    // Same desktop Chrome signature rutor has been sending -- a mobile
    // or library UA on an HTML endpoint is another bot signal.
    let user_agent = concat!(
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) ",
        "Chrome/152.0.0.0 Safari/537.36"
    );
    Client::builder()
        .user_agent(user_agent)
        .default_headers(headers)
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// Parse `Retry-After` (torio's `parseRetryAfter`): either delta-seconds or an HTTP date, both
/// returned as milliseconds relative to `now_ms`.
pub fn parse_retry_after(value: Option<&str>, now_ms: i64) -> Option<u64> {
    let raw = value?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(seconds) = raw.parse::<u64>() {
        // `^\d+$` in torio: only pure digit strings are delta-seconds.
        if raw.chars().all(|c| c.is_ascii_digit()) {
            return Some(seconds.saturating_mul(1000));
        }
        return None;
    }
    // An HTTP date (RFC 2822) is relative to *now*, and never negative:
    // a date already in the past means "retry immediately".
    let parsed = chrono::DateTime::parse_from_rfc2822(raw).ok()?;
    let delay = parsed.timestamp_millis().saturating_sub(now_ms);
    Some(delay.try_into().unwrap_or(0))
}

/// Backoff before `attempt` + 1 (torio's `backoffDelay`): a random point in `[0, min(cap, base
/// * 2^attempt))`, floored -- never undercut -- by `Retry-After` when the server named one.
pub fn backoff_delay(
    attempt: u32,
    base_ms: u64,
    cap_ms: u64,
    retry_after_ms: Option<u64>,
    rand: f64,
) -> u64 {
    let exp = base_ms.saturating_mul(1u64 << attempt.min(30));
    let exp = exp.min(cap_ms);
    let jittered = (rand.clamp(0.0, 1.0) * exp as f64) as u64;
    match retry_after_ms {
        Some(floor) => jittered.max(floor),
        None => jittered,
    }
}

/// Send the request built by `build`, retrying transient failures: network errors and
/// [`RETRY_STATUS`] responses get up to `opts.retries` more attempts, each preceded by
/// [`backoff_delay`].
pub async fn fetch_resilient<F>(
    url: &str,
    build: F,
    opts: &FetchOptions,
) -> Result<reqwest::Response>
where
    F: Fn() -> RequestBuilder,
{
    let mut attempt = 0u32;
    loop {
        let response = match build().send().await {
            Ok(response) => response,
            Err(err) => {
                // Transport-level failure (refused, reset, timeout):
                // worth another try until the budget runs out.
                if attempt >= opts.retries {
                    anyhow::bail!("GET {} failed after {} retries: {}", url, opts.retries, err);
                }
                let delay = opts.delay(attempt, None);
                tokio::time::sleep(Duration::from_millis(delay)).await;
                attempt += 1;
                continue;
            }
        };

        let status = response.status().as_u16();
        if !is_retryable(status) {
            return Ok(response);
        }

        let server = response
            .headers()
            .get(SERVER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if status == 503 && (server.contains("ddos-guard") || server.contains("cloudflare")) {
            anyhow::bail!(
                "Request to {} blocked by {} (HTTP {}).",
                url,
                server,
                status
            );
        }

        if attempt >= opts.retries {
            anyhow::bail!(
                "Request to {} failed after {} retries (HTTP {}).",
                url,
                opts.retries,
                status
            );
        }

        let retry_after = parse_retry_after(
            response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok()),
            chrono::Utc::now().timestamp_millis(),
        );
        let delay = opts.delay(attempt, retry_after);
        tokio::time::sleep(Duration::from_millis(delay)).await;
        attempt += 1;
    }
}

/// Uniform-ish random in `[0, 1)` for the backoff jitter: sub-second microseconds, so two waits
/// in the same microsecond do not line up.
fn rand_fraction() -> f64 {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_micros())
        .unwrap_or(0);
    micros as f64 / 1_000_000.0
}

/// Try `bases` in order and hand back the first success ( failover helper, deliberately
/// deferred until a source actually had mirrors to fail over to -- yts is the first, B8 wave
/// 1).
pub async fn first_ok<T, F, Fut>(bases: &[&str], attempt: F) -> Result<T>
where
    F: Fn(&str) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    if bases.is_empty() {
        anyhow::bail!("no hosts to try");
    }
    let mut last_error = None;
    for base in bases {
        match attempt(base).await {
            Ok(value) => return Ok(value),
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("no host answered")))
}
