use std::sync::{Arc, Mutex};

use doris::torrserver::api::{TorrServer, TorrentInfo};

// The *older* shape: Go structs with no `json` tags, so the output uses
// the exact capitalized Go field names. `TorrentInfo` must keep reading
// this -- but it is no longer the only shape out there: see
// `LIVE_LIST_JSON` below and the doc comment on TorrentInfo for why
// accepting only this one silently emptied the Torrent zone.
const SAMPLE_LIST_JSON: &str = r#"
[
  {
    "Name": "Big Buck Bunny",
    "Hash": "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c",
    "TorrentSize": 1000000000,
    "LoadedSize": 250000000,
    "DownloadSpeed": 1048576.0,
    "UploadSpeed": 65536.0,
    "TotalPeers": 12,
    "ActivePeers": 5,
    "ConnectedSeeders": 3,
    "TorrentStatusString": "Downloading"
  }
]
"#;

#[test]
fn test_parse_torrent_list_response() {
    let list: Vec<TorrentInfo> = serde_json::from_str(SAMPLE_LIST_JSON).unwrap();
    assert_eq!(list.len(), 1);
    let t = &list[0];
    assert_eq!(t.name, "Big Buck Bunny");
    assert_eq!(t.hash, "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c");
    assert_eq!(t.total_size, 1_000_000_000);
    assert_eq!(t.loaded_size, 250_000_000);
    assert_eq!(t.total_peers, 12);
    assert_eq!(t.status_string, "Downloading");
    assert!((t.progress() - 0.25).abs() < 1e-9);
}

#[test]
fn test_parse_empty_torrent_list() {
    let list: Vec<TorrentInfo> = serde_json::from_str("[]").unwrap();
    assert!(list.is_empty());
}

#[test]
fn test_torrent_info_missing_fields_default_instead_of_failing() {
    // A fork with a slightly different shape (extra or missing fields)
    // should still parse -- unknown fields are ignored by default, and
    // every field on TorrentInfo has #[serde(default)].
    let json = r#"{"Hash": "abc123", "SomeExtraField": 42}"#;
    let t: TorrentInfo = serde_json::from_str(json).unwrap();
    assert_eq!(t.hash, "abc123");
    assert_eq!(t.total_size, 0);
    assert_eq!(t.progress(), 0.0);
}

#[test]
fn test_progress_clamped_and_no_division_by_zero() {
    let t = TorrentInfo { total_size: 0, loaded_size: 500, ..Default::default() };
    assert_eq!(t.progress(), 0.0);

    let t = TorrentInfo { total_size: 100, loaded_size: 200, ..Default::default() };
    assert_eq!(t.progress(), 1.0);
}

// --- Mock-server integration tests for TorrServer's actual API calls ------
//
// These spin up a tiny hand-rolled HTTP server over a raw TcpListener
// (no mocking crate needed -- reqwest, tokio, and serde_json are already
// project dependencies) so list_torrents/get_torrent/pause/resume/remove/
// is_reachable are exercised end-to-end against a real socket, not just
// unit-tested at the JSON-parsing layer like the tests above. Each test
// gets its own server on an OS-assigned port so they can run in parallel
// without colliding.

/// Start a minimal HTTP/1.1 server that responds to every request with the
/// same canned `status_line` + `body`, on a freshly assigned localhost
/// port. Good enough to exercise a client's request/response handling
/// without needing to actually parse the incoming request -- none of the
/// TorrServer client methods branch on anything in the request itself.
async fn spawn_mock_server(status_line: &'static str, body: &'static str) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind mock server");
    let addr = listener.local_addr().expect("mock server local addr");
    let handle = tokio::spawn(async move {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => break,
            };
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 8192];
                // We don't need to parse the request -- none of these
                // tests depend on it -- just drain it so the client isn't
                // left waiting on a write that never gets read.
                let _ = socket.read(&mut buf).await;
                let response = format!(
                    "{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status_line,
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (format!("http://{}", addr), handle)
}

#[tokio::test]
async fn test_is_reachable_true_on_200() {
    let (url, handle) = spawn_mock_server("HTTP/1.1 200 OK", "{}").await;
    let client = TorrServer::new(&url);
    assert!(client.is_reachable().await);
    handle.abort();
}

#[tokio::test]
async fn test_is_reachable_false_when_nothing_listening() {
    // Port 1 is reserved and essentially guaranteed not to have anything
    // listening on it in a test environment.
    let client = TorrServer::new("http://127.0.0.1:1");
    assert!(!client.is_reachable().await);
}

#[tokio::test]
async fn test_list_torrents_parses_real_response_over_the_wire() {
    let (url, handle) = spawn_mock_server("HTTP/1.1 200 OK", SAMPLE_LIST_JSON).await;
    let client = TorrServer::new(&url);
    let list = client.list_torrents().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "Big Buck Bunny");
    assert_eq!(list[0].hash, "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c");
    handle.abort();
}

#[tokio::test]
async fn test_list_torrents_treats_null_response_as_empty() {
    // TorrServer returns bare `null`, not `[]`, when there are no
    // torrents -- list_torrents() must not error on that.
    let (url, handle) = spawn_mock_server("HTTP/1.1 200 OK", "null").await;
    let client = TorrServer::new(&url);
    let list = client.list_torrents().await.unwrap();
    assert!(list.is_empty());
    handle.abort();
}

#[tokio::test]
async fn test_list_torrents_survives_repeated_calls_against_same_server() {
    let (url, handle) = spawn_mock_server("HTTP/1.1 200 OK", SAMPLE_LIST_JSON).await;
    let client = TorrServer::new(&url);
    for _ in 0..3 {
        let list = client.list_torrents().await.unwrap();
        assert_eq!(list.len(), 1);
    }
    handle.abort();
}

#[tokio::test]
async fn test_get_torrent_returns_none_on_error_status() {
    let (url, handle) = spawn_mock_server("HTTP/1.1 404 Not Found", "{}").await;
    let client = TorrServer::new(&url);
    let result = client.get_torrent("somehash").await.unwrap();
    assert!(result.is_none());
    handle.abort();
}

#[tokio::test]
async fn test_get_torrent_returns_some_on_success() {
    let single = r#"{"Name": "Test Torrent", "Hash": "abc123"}"#;
    let (url, handle) = spawn_mock_server("HTTP/1.1 200 OK", single).await;
    let client = TorrServer::new(&url);
    let result = client.get_torrent("abc123").await.unwrap();
    let info: TorrentInfo = result.expect("expected Some(TorrentInfo)");
    assert_eq!(info.hash, "abc123");
    assert_eq!(info.name, "Test Torrent");
    handle.abort();
}

#[tokio::test]
async fn test_pause_succeeds_on_200() {
    let (url, handle) = spawn_mock_server("HTTP/1.1 200 OK", "{}").await;
    let client = TorrServer::new(&url);
    assert!(client.pause("somehash").await.is_ok());
    handle.abort();
}

#[tokio::test]
async fn test_pause_does_not_error_on_non_2xx() {
    // Documents current behavior: pause()/remove() only propagate an
    // Err for transport-level failures (connection refused, timeout),
    // not HTTP error statuses -- reqwest doesn't treat 4xx/5xx as Err
    // unless .error_for_status() is called, which these methods don't.
    let (url, handle) = spawn_mock_server("HTTP/1.1 500 Internal Server Error", "{}").await;
    let client = TorrServer::new(&url);
    assert!(client.pause("somehash").await.is_ok());
    handle.abort();
}

#[tokio::test]
async fn test_resume_succeeds_on_200() {
    let (url, handle) = spawn_mock_server("HTTP/1.1 200 OK", r#"{"Hash":"x"}"#).await;
    let client = TorrServer::new(&url);
    assert!(client.resume("x").await.is_ok());
    handle.abort();
}

#[tokio::test]
async fn test_remove_succeeds_on_200() {
    let (url, handle) = spawn_mock_server("HTTP/1.1 200 OK", "{}").await;
    let client = TorrServer::new(&url);
    assert!(client.remove("somehash").await.is_ok());
    handle.abort();
}

#[tokio::test]
async fn test_is_reachable_and_list_torrents_share_a_client_correctly() {
    // Exercises the same TorrServer instance for two different call
    // shapes (bare GET vs POST with a JSON body) against the same
    // server, matching how the app::App orchestrator actually uses one
    // long-lived TorrServer client for the whole session.
    let (url, handle) = spawn_mock_server("HTTP/1.1 200 OK", SAMPLE_LIST_JSON).await;
    let client = TorrServer::new(&url);
    assert!(client.is_reachable().await);
    let list = client.list_torrents().await.unwrap();
    assert_eq!(list.len(), 1);
    handle.abort();
}

// --- add_by_link (B7) --------------------------------------------------------

/// Unlike the mock above, this one *records* what it was asked -- the
/// whole point of `add_by_link`'s tests is the shape of the request body,
/// which is specified in TorrServer's `torrReqJS`. Returns
/// `"<request line>\n<body>"` once a request has been seen.
async fn spawn_recording_server(
    status_line: &'static str,
    response_body: &'static str,
) -> (String, Arc<Mutex<Option<String>>>, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock server");
    let addr = listener.local_addr().expect("mock server local addr");
    let record = Arc::new(Mutex::new(None));
    let recorder = Arc::clone(&record);
    let handle = tokio::spawn(async move {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(v) => v,
                Err(_) => break,
            };
            let mut buf: Vec<u8> = Vec::new();
            let mut chunk = [0u8; 4096];
            // Read through the header terminator first...
            loop {
                let at_header_end = buf.windows(4).any(|w| w == b"\r\n\r\n");
                if at_header_end {
                    break;
                }
                match socket.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            }
            let head_end = buf
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|p| p + 4)
                .unwrap_or(buf.len());
            let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
            // ... then exactly Content-Length bytes of body.
            let content_length: usize = head
                .lines()
                .find_map(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().to_string())
                })
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let mut body = buf[head_end..].to_vec();
            while body.len() < content_length {
                match socket.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => body.extend_from_slice(&chunk[..n]),
                }
            }
            *recorder.lock().expect("record lock") = Some(format!(
                "{}\n{}",
                head.lines().next().unwrap_or_default(),
                String::from_utf8_lossy(&body)
            ));
            let response = format!(
                "{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status_line,
                response_body.len(),
                response_body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    (format!("http://{}", addr), record, handle)
}

const TEST_MAGNET: &str =
    "magnet:?xt=urn:btih:abcdef0123456789abcdef0123456789abcdef01&dn=Big+Buck+Bunny";

#[tokio::test]
async fn test_add_by_link_posts_the_documented_body_and_returns_the_hash() {
    let (url, record, handle) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"{"hash":"deadbeef"}"#).await;
    let torrserver = TorrServer::new(&url);

    let hash = torrserver
        .add_by_link(TEST_MAGNET, "Big Buck Bunny")
        .await
        .expect("a 200 with a hash is success");

    assert_eq!(hash, "deadbeef");
    let recorded = record.lock().expect("record lock").clone().expect("one request");
    let (request_line, body) = recorded.split_once('\n').expect("head + body");
    assert_eq!(request_line, "POST /torrents HTTP/1.1");

    let sent: serde_json::Value = serde_json::from_str(body).expect("a JSON body");
    assert_eq!(sent["action"], "add");
    assert_eq!(sent["link"], TEST_MAGNET);
    assert_eq!(sent["title"], "Big Buck Bunny");
    assert_eq!(
        sent["save_to_db"], true,
        "kept symmetric with /torrent/upload?save=db"
    );
    handle.abort();
}

#[tokio::test]
async fn test_add_by_link_surfaces_the_reason_torrserver_rejected_the_link() {
    // What TorrServer actually answers for an unparseable link (its
    // `addTorrent` handler aborts with this exact body).
    let (url, _record, handle) = spawn_recording_server(
        "HTTP/1.1 400 Bad Request",
        r#"{"error":"error parse link: not a magnet"}"#,
    )
    .await;
    let torrserver = TorrServer::new(&url);

    let err = torrserver
        .add_by_link("definitely-not-a-link", "whatever")
        .await
        .expect_err("a 400 must fail");

    assert!(
        err.to_string().contains("error parse link"),
        "the reason must reach the log: {}",
        err
    );
    handle.abort();
}

#[tokio::test]
async fn test_add_by_link_requires_a_hash_back() {
    // A 200 without a usable hash is a caller that cannot proceed: the
    // stream needs something to play by.
    let (url, _record, handle) = spawn_recording_server("HTTP/1.1 200 OK", "{}").await;
    let torrserver = TorrServer::new(&url);

    let err = torrserver
        .add_by_link(TEST_MAGNET, "Big Buck Bunny")
        .await
        .expect_err("no hash, no way to stream");

    assert!(err.to_string().contains("No hash"), "{}", err);
    handle.abort();
}

// The shape the *running* server answered with on 25.09.2026 (`state.
// TorrentStatus`, json-tagged). Captured from a live `{"action":"list"}`
// -- row 1 verbatim, row 2 reconstructed so the fields that upstream
// marks `omitempty` (and therefore drops when they are zero) are covered
// too.
const LIVE_LIST_JSON: &str = r#"
[
  {
    "title": "Колония - Gunche - Colony (2026) WEB-DL 1080p.mkv",
    "category": "",
    "poster": "",
    "timestamp": 1789839446,
    "hash": "b2fb4854cf32921561786c987642e007cb9f279f",
    "stat": 5,
    "stat_string": "Torrent in db",
    "torrent_size": 7626028452
  },
  {
    "title": "Some.Torrent.2026.1080p.mkv",
    "category": "",
    "poster": "",
    "timestamp": 1789839446,
    "hash": "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c",
    "stat": 2,
    "stat_string": "Torrent working",
    "loaded_size": 250000000,
    "torrent_size": 1000000000,
    "download_speed": 1048576.0,
    "upload_speed": 65536.0,
    "total_peers": 12,
    "active_peers": 5,
    "connected_seeders": 3
  }
]
"#;

/// The bug this sample pins: against a modern (json-tagged) server the
/// capitalized-only renames matched nothing, `#[serde(default)]` filled
/// in zeros, and the Torrent zone came back with no name, no hash, no
/// size, no speed and no status -- silently, because nothing errored.
#[test]
fn test_parses_the_tagged_shape_a_modern_torrserver_actually_sends() {
    let list: Vec<TorrentInfo> = serde_json::from_str(LIVE_LIST_JSON).unwrap();
    assert_eq!(list.len(), 2);

    let first = &list[0];
    assert_eq!(first.name, "Колония - Gunche - Colony (2026) WEB-DL 1080p.mkv");
    assert_eq!(first.hash, "b2fb4854cf32921561786c987642e007cb9f279f");
    assert_eq!(first.total_size, 7626028452);
    assert_eq!(first.status_string, "Torrent in db");
    assert_eq!(first.loaded_size, 0, "absent keys still default");

    let second = &list[1];
    assert_eq!(second.hash, "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c");
    assert_eq!(second.total_peers, 12);
    assert_eq!(second.connected_seeders, 3);
    assert_eq!(second.status_string, "Torrent working");
    assert_eq!(second.progress(), 0.25, "loaded/total from the tagged keys");
}

/// Progress drives the sparkline, so the tagged keys feeding it must not
/// silently read as zero.
#[test]
fn test_progress_from_the_tagged_shape_is_not_always_zero() {
    let list: Vec<TorrentInfo> = serde_json::from_str(LIVE_LIST_JSON).unwrap();

    assert_eq!(list[0].progress(), 0.0, "no loaded_size in that row");
    assert_eq!(list[1].progress(), 0.25);
    assert_eq!(list[1].download_speed, 1048576.0);
}

/// The older, capitalized shape must keep working -- some installs (and
/// forks) still answer with it.
#[test]
fn test_the_legacy_capitalized_shape_still_parses() {
    let list: Vec<TorrentInfo> = serde_json::from_str(SAMPLE_LIST_JSON).unwrap();

    assert_eq!(list[0].name, "Big Buck Bunny");
    assert_eq!(list[0].hash, "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c");
    assert_eq!(list[0].total_size, 1000000000);
    assert_eq!(list[0].loaded_size, 250000000);
    assert_eq!(list[0].status_string, "Downloading");
    assert_eq!(list[0].progress(), 0.25);
}
