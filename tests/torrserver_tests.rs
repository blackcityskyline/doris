use doris::torrserver::api::{TorrServer, TorrentInfo};

// TorrServer's Go structs have no `json` tags, so its default JSON output
// uses the exact (capitalized) Go field names. This test locks in that
// assumption -- see the doc comment on TorrentInfo for sources and the
// caveat that some forks may differ slightly. If a real TorrServer
// instance's `/torrents` response doesn't match this shape, this is the
// test (and the #[serde(rename = ...)] list in torrserver/api.rs) to fix.
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
