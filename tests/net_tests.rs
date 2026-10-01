//! Mock-server tests for `search/net.rs` -- torio's `net.test.ts`
//! re-expressed over a real HTTP socket, using the `spawn_mock_server`
//! pattern from `tests/torrserver_tests.rs`.
//!
//! What matters here is *how many times* the server was asked, and after
//! how long: retrying is the whole feature, and retrying a challenge
//! page (the one case that must not happen) is the whole risk.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use doris::sources::net::{
    backoff_delay, fetch_resilient, first_ok, is_retryable, parse_retry_after, FetchOptions,
};

/// Serves each connection with the next entry of `script`; the last one repeats forever, so a
/// test can say "always 503".
async fn spawn_scripted(
    script: Vec<String>,
) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    assert!(!script.is_empty(), "an empty script would answer nothing");
    let hits = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock server");
    let addr = listener.local_addr().expect("mock server local addr");
    let counter = Arc::clone(&hits);
    let handle = tokio::spawn(async move {
        let mut served = 0usize;
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => break,
            };
            let response = script[served.min(script.len() - 1)].clone();
            served += 1;
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 8192];
                let _ = socket.read(&mut buf).await;
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (format!("http://{}", addr), hits, handle)
}

fn response(status_line: &str, headers: &[(&str, &str)], body: &str) -> String {
    let mut out = format!("{}\r\n", status_line);
    for (name, value) in headers {
        out.push_str(&format!("{}: {}\r\n", name, value));
    }
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    ));
    out
}

/// Small enough that the backoff between attempts is imperceptible; the
/// one test that *cares* about the wait sets its own values.
fn fast_options(retries: u32) -> FetchOptions {
    FetchOptions {
        retries,
        base_ms: 1,
        cap_ms: 1,
    }
}

fn hits(counter: &Arc<AtomicUsize>) -> usize {
    counter.load(Ordering::SeqCst)
}

#[tokio::test]
async fn test_retries_a_503_then_returns_the_success_response() {
    let (url, counter, handle) = spawn_scripted(vec![
        response("HTTP/1.1 503 Service Unavailable", &[], ""),
        response("HTTP/1.1 200 OK", &[], "ok"),
    ])
    .await;
    let client = reqwest::Client::new();

    let got = fetch_resilient(&url, || client.get(&url), &fast_options(3))
        .await
        .expect("the second attempt must succeed");

    assert_eq!(got.status().as_u16(), 200);
    assert_eq!(hits(&counter), 2, "exactly one retry, then the answer");
    handle.abort();
}

#[tokio::test]
async fn test_gives_up_after_the_configured_retries() {
    // One entry repeats forever: the server never stops saying 503.
    let (url, counter, handle) =
        spawn_scripted(vec![response("HTTP/1.1 503 Service Unavailable", &[], "")]).await;
    let client = reqwest::Client::new();

    let err = fetch_resilient(&url, || client.get(&url), &fast_options(2))
        .await
        .expect_err("the budget must run out");

    let message = err.to_string();
    assert!(
        message.contains("failed after 2 retries"),
        "error must say how long it tried: {}",
        message
    );
    assert_eq!(hits(&counter), 3, "initial attempt + 2 retries");
    handle.abort();
}

#[tokio::test]
async fn test_a_ddos_guard_503_is_not_retried() {
    // The bug this whole module exists to prevent: a challenge page is
    let (url, counter, handle) = spawn_scripted(vec![response(
        "HTTP/1.1 503 Service Unavailable",
        &[("Server", "ddos-guard")],
        "<html>challenge</html>",
    )])
    .await;
    let client = reqwest::Client::new();

    let err = fetch_resilient(&url, || client.get(&url), &fast_options(5))
        .await
        .expect_err("a challenge must fail loudly");

    let message = err.to_string();
    assert!(
        message.contains("blocked by ddos-guard"),
        "expected the challenge complaint, got: {}",
        message
    );
    assert_eq!(hits(&counter), 1, "one request total: no retries at all");
    handle.abort();
}

#[tokio::test]
async fn test_a_non_retryable_status_is_handed_back_to_the_caller() {
    let (url, counter, handle) =
        spawn_scripted(vec![response("HTTP/1.1 404 Not Found", &[], "gone")]).await;
    let client = reqwest::Client::new();

    let got = fetch_resilient(&url, || client.get(&url), &fast_options(5))
        .await
        .expect("404 is an answer, not a failure");

    assert_eq!(got.status().as_u16(), 404);
    assert_eq!(hits(&counter), 1, "a wrong answer must not be retried");
    handle.abort();
}

#[tokio::test]
async fn test_retry_after_is_a_floor_not_a_suggestion() {
    let (url, counter, handle) = spawn_scripted(vec![response(
        "HTTP/1.1 503 Service Unavailable",
        &[("Retry-After", "1")],
        "",
    )])
    .await;
    let client = reqwest::Client::new();

    let started = Instant::now();
    let err = fetch_resilient(&url, || client.get(&url), &fast_options(1))
        .await
        .expect_err("still 503 on the second attempt");
    let waited = started.elapsed();

    assert!(err.to_string().contains("failed after 1 retries"));
    assert_eq!(hits(&counter), 2);
    assert!(
        waited >= Duration::from_millis(900),
        "the 1s Retry-After must have been honored, waited only {:?}",
        waited
    );
    handle.abort();
}

// --- pure helpers (torio's net.test.ts cases) -------------------------------

#[test]
fn test_parse_retry_after_parses_delta_seconds_and_rejects_garbage() {
    assert_eq!(parse_retry_after(Some("5"), 0), Some(5000));
    assert_eq!(parse_retry_after(None, 0), None);
    assert_eq!(parse_retry_after(Some("soon"), 0), None);
    assert_eq!(parse_retry_after(Some(""), 0), None);
    assert_eq!(parse_retry_after(Some("-5"), 0), None, "not delta-seconds");
}

#[test]
fn test_parse_retry_after_reads_an_http_date_relative_to_now() {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let future = (chrono::Utc::now() + chrono::Duration::seconds(10)).to_rfc2822();

    let delay = parse_retry_after(Some(&future), now_ms).expect("a date parses");

    assert!(delay > 5000, "must be counted from now, got {}ms", delay);
    assert!(
        delay <= 10_000,
        "must not overshoot the date, got {}ms",
        delay
    );
}

#[test]
fn test_parse_retry_after_never_goes_negative_for_a_date_in_the_past() {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let past = (chrono::Utc::now() - chrono::Duration::seconds(30)).to_rfc2822();

    assert_eq!(parse_retry_after(Some(&past), now_ms), Some(0));
}

#[test]
fn test_backoff_delay_jitters_and_treats_retry_after_as_a_floor() {
    // torio's net.test.ts cases, same inputs and expectations.
    assert_eq!(backoff_delay(0, 500, 20_000, None, 0.5), 250);
    assert_eq!(backoff_delay(2, 500, 20_000, None, 0.0), 0);
    assert_eq!(backoff_delay(0, 500, 20_000, Some(1000), 0.5), 1000);
    // The ceiling holds however far the exponential would have run.
    assert_eq!(backoff_delay(30, 500, 20_000, None, 1.0), 20_000);
}

#[test]
fn test_only_torios_retryable_statuses_are_retried() {
    for status in [408, 425, 429, 500, 502, 503, 504] {
        assert!(is_retryable(status), "{} should be retried", status);
    }
    for status in [200, 301, 400, 403, 404, 418, 451] {
        assert!(!is_retryable(status), "{} should not be retried", status);
    }
}

// --- first_ok ( failover helper, first consumer yts) ---------------------

/// What a multi-host source asks one host: GET it, and treat a non-2xx as a failure so the next
/// host gets its turn.
async fn ask(client: reqwest::Client, base: String) -> anyhow::Result<u16> {
    let resp = client
        .get(&base)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    anyhow::ensure!(
        resp.status().is_success(),
        "{}: HTTP {}",
        base,
        resp.status()
    );
    Ok(resp.status().as_u16())
}

#[tokio::test]
async fn test_first_ok_falls_through_to_the_next_host() {
    // Nothing listens on port 1, so the first attempt fails at connect
    let (url, counter, handle) =
        spawn_scripted(vec![response("HTTP/1.1 200 OK", &[], "pong")]).await;
    let client = reqwest::Client::new();
    let attempt = |base: &str| ask(client.clone(), base.to_string());

    let status = first_ok(&["http://127.0.0.1:1", url.as_str()], attempt)
        .await
        .expect("the second host must win");

    assert_eq!(status, 200);
    assert_eq!(hits(&counter), 1, "only the working host is ever asked");
    handle.abort();
}

#[tokio::test]
async fn test_first_ok_stops_at_the_first_success() {
    let (ok_url, ok_hits, ok_handle) =
        spawn_scripted(vec![response("HTTP/1.1 200 OK", &[], "ok")]).await;
    let (never_url, never_hits, never_handle) = spawn_scripted(vec![response(
        "HTTP/1.1 500 Internal Server Error",
        &[],
        "",
    )])
    .await;
    let client = reqwest::Client::new();
    let attempt = |base: &str| ask(client.clone(), base.to_string());

    let status = first_ok(&[ok_url.as_str(), never_url.as_str()], attempt)
        .await
        .expect("the first host answers");

    assert_eq!(status, 200);
    assert_eq!(hits(&ok_hits), 1);
    assert_eq!(
        hits(&never_hits),
        0,
        "a working first host must never cost a request to the second"
    );
    ok_handle.abort();
    never_handle.abort();
}

#[tokio::test]
async fn test_first_ok_surfaces_the_last_error_when_every_host_fails() {
    let client = reqwest::Client::new();
    let attempt = |base: &str| ask(client.clone(), base.to_string());

    let err = first_ok(&["http://127.0.0.1:1", "http://127.0.0.1:2"], attempt)
        .await
        .expect_err("nothing is listening");

    // torio keeps `lastError` too: the final host's error describes the
    assert!(
        err.to_string().contains("127.0.0.1:2"),
        "expected the *last* host's error, got: {}",
        err
    );
}

#[tokio::test]
async fn test_first_ok_rejects_an_empty_host_list() {
    let err = first_ok::<u8, _, _>(&[], |_base| async { Ok(0u8) })
        .await
        .expect_err("no hosts is a caller bug, not a network condition");

    assert!(err.to_string().contains("no hosts"), "{}", err);
}
