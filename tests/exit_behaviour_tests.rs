//! What happens to the download when doris exits.
//!
//! "Close torrent core on exit" was an Options row that persisted a value
//! nothing read: doris left and the transfer carried on on TorrServer
//! forever, which is the opposite of what the label says.
//!
//! What it must do is *pause*, not remove: `remove` would delete the
//! user's data on the way out of the program they were watching it with.
//! So this checks the bytes that go over the wire, not just which hash was
//! chosen: the server records its body and the assertion is on the
//! `action` in it. `drop` is the pause; `rem` is the delete, and a
//! regression from one to the other would not fail any other test here.

use doris::app::{stop_download_on_exit, stop_the_download};
use doris::config::Config;
use doris::torrserver::api::TorrServer;
use std::sync::{Arc, Mutex};

fn on(close_on_exit: bool) -> Config {
    Config {
        close_torrent_core_on_exit: close_on_exit,
        ..Default::default()
    }
}

#[test]
fn the_download_stops_when_the_option_is_on() {
    assert_eq!(
        stop_download_on_exit(&on(true), Some("abc123")),
        Some("abc123"),
        "the tracked torrent is the one to stop"
    );
}

#[test]
fn the_download_keeps_going_when_the_option_is_off() {
    assert_eq!(
        stop_download_on_exit(&on(false), Some("abc123")),
        None,
        "turning the option off must mean the transfer is left alone"
    );
}

#[test]
fn nothing_to_stop_is_not_an_error() {
    assert_eq!(
        stop_download_on_exit(&on(true), None),
        None,
        "a run that never played anything has nothing to stop"
    );
    assert_eq!(stop_download_on_exit(&on(false), None), None);
}

/// A server that answers 200 and remembers what it was sent.
async fn recording_server() -> (String, Arc<Mutex<Vec<String>>>) {
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let sink = Arc::clone(&sink);
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                sink.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..n]).into_owned());
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                    )
                    .await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (url, seen)
}

#[tokio::test]
async fn stopping_on_exit_pauses_and_never_removes() {
    let (url, seen) = recording_server().await;
    let client = TorrServer::new(&url);

    // The *call* the exit path makes, not a re-typing of it: the first
    // draft of this test called `client.pause` directly, and swapping the
    // exit path to `remove` left it green.
    stop_the_download(&on(true), Some("abc123"), &client).await;

    let bodies = seen.lock().unwrap().clone();
    assert_eq!(bodies.len(), 1, "one request, not two:\n{bodies:?}");
    assert!(
        bodies[0].contains("\"drop\""),
        "must be a pause (drop), not a delete: {}",
        bodies[0]
    );
    assert!(
        !bodies[0].contains("\"rem\""),
        "removing here would delete the user's file on the way out: {}",
        bodies[0]
    );
    assert!(
        bodies[0].contains("abc123"),
        "and it must be the tracked one: {}",
        bodies[0]
    );
}

#[tokio::test]
async fn the_option_off_sends_nothing_at_all() {
    let (url, seen) = recording_server().await;
    let client = TorrServer::new(&url);

    stop_the_download(&on(false), Some("abc123"), &client).await;

    assert!(
        seen.lock().unwrap().is_empty(),
        "nothing may be sent when the option is off: {:?}",
        seen.lock().unwrap()
    );
}

#[tokio::test]
async fn a_failure_to_stop_is_reported_not_swallowed() {
    // The server is not running: the exit path must not turn a failed
    // stop into silence -- a user who asked for "stop on exit" and got
    // a transfer still going deserves to be told.
    let client = TorrServer::new("http://127.0.0.1:1");
    let message = stop_the_download(&on(true), Some("abc123"), &client)
        .await
        .expect("the outcome is reported");
    assert!(
        message.contains("Could not stop"),
        "a failure must reach the user: {message}"
    );
}

#[tokio::test]
async fn a_successful_stop_says_so() {
    let (url, _seen) = recording_server().await;
    let client = TorrServer::new(&url);

    let message = stop_the_download(&on(true), Some("abc123"), &client)
        .await
        .expect("the outcome is reported");

    assert!(message.contains("abc123"), "{message}");
}
