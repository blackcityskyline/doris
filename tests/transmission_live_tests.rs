use doris::transmission::{Added, Transmission};

/// These talk to a real `transmission-daemon`. They are `#[ignore]`d for
/// the same reason the tracker's live probes are: they need a service that
/// is not part of the test run.
///
///     cargo test --test transmission_live_tests -- --ignored --nocapture
///     TORRENT_URL=http://127.0.0.1:9091 \
///       cargo test --test transmission_live_tests -- --ignored
///
/// The magnet is a public test file, not anything of the author's, so a
/// run cannot be mistaken for use of a tracker.
fn url() -> String {
    std::env::var("TORRENT_URL").unwrap_or_else(|_| "http://127.0.0.1:9091".into())
}

/// The handshake is the part of Transmission's protocol a client can get
/// wrong on a perfectly healthy daemon. If `version` answers, the 409 dance
/// worked.
#[tokio::test]
#[ignore]
async fn a_healthy_daemon_answers_after_the_session_handshake() {
    let t = Transmission::new(&url());
    assert!(
        t.is_reachable().await,
        "no Transmission at {}",
        t.base_url()
    );
    let version = t.version().await.expect("session-get");
    assert!(version.contains('.'), "not a version: {version:?}");
    assert!(t.free_space().await.expect("free space") > 0);
}

/// Pausing has to be checked by the *state*, never by the answer.
///
/// `torrent-set` with `action: "stop"` answers `result: success` on a
/// Transmission 4 and changes nothing: the status stays `4` and the
/// download goes on. A client written against the documented modern call
/// therefore reports "paused" over a torrent that is still downloading.
/// This test is the one that catches it, and it is here because the
/// failure mode is invisible from the client side -- which is exactly why
/// it survived being written.
#[tokio::test]
#[ignore]
async fn pausing_changes_the_state_and_not_only_the_answer() {
    let t = Transmission::new(&url());
    let magnet = std::env::var("TEST_MAGNET")
        .unwrap_or_else(|_| "magnet:?xt=urn:btih:045e85f2ebc24a875a64fe2e9ac9b61f7aad0499".into());
    let dir = std::env::var("TEST_DIR").unwrap_or_else(|_| "/tmp/doris-transmission-test".into());

    let id = match t.add(&magnet, Some(&dir)).await.expect("add") {
        Added::Fresh(id) | Added::AlreadyThere(id) => id,
        Added::Refused(why) => panic!("refused: {why}"),
    };

    t.resume(id).await.expect("resume");
    let running = t.get(id).await.expect("get").expect("held");
    assert_ne!(
        running.status, 0,
        "it was never started, so stopping proves nothing"
    );

    t.pause(id).await.expect("pause");
    // Waited for rather than slept on: Transmission applies a stop on its
    // own schedule, so asserting the instant the call returns would be a
    // test of the sleep length. `watch` is the same wait the panel uses, so
    // this exercises the code that ships rather than a copy of it.
    let stopped = t
        .watch(id, std::time::Duration::from_secs(10), |d| d.status == 0)
        .await
        .expect("watch")
        .expect("held");
    assert_eq!(
        stopped.status,
        0,
        "pause answered successfully and the torrent is still {:?}",
        stopped.state()
    );

    t.resume(id).await.expect("resume again");
    let back = t
        .watch(id, std::time::Duration::from_secs(10), |d| d.status != 0)
        .await
        .expect("watch")
        .expect("held");
    assert_ne!(back.status, 0, "resume did not start it either");

    t.remove(id, true).await.expect("remove");
}

/// `add` distinguishes three outcomes and the middle one is the one a
/// caller reports as a failure when it is not: a magnet the daemon already
/// holds.
#[tokio::test]
#[ignore]
async fn adding_the_same_twice_is_not_an_error() {
    let t = Transmission::new(&url());
    let magnet = std::env::var("TEST_MAGNET")
        .unwrap_or_else(|_| "magnet:?xt=urn:btih:045e85f2ebc24a875a64fe2e9ac9b61f7aad0499".into());
    let dir = std::env::var("TEST_DIR").unwrap_or_else(|_| "/tmp/doris-transmission-test".into());

    let first = t.add(&magnet, Some(&dir)).await.expect("first add");
    let second = t.add(&magnet, Some(&dir)).await.expect("second add");
    assert!(
        matches!(first, Added::Fresh(_) | Added::AlreadyThere(_)),
        "first add refused: {first:?}"
    );
    let id = match (first, second) {
        (Added::Fresh(a), _) | (_, Added::Fresh(a)) => a,
        (Added::AlreadyThere(a), Added::AlreadyThere(b)) => {
            assert_eq!(a, b, "the same magnet came back as two downloads");
            a
        }
        (first, second) => panic!("fresh or the same duplicate, got {first:?} / {second:?}"),
    };

    // Whatever happened, it is findable by hash -- the join the whole
    // adopt feature rests on.
    let found = t
        .find_by_hash("045e85f2ebc24a875a64fe2e9ac9b61f7aad0499")
        .await
        .expect("lookup")
        .expect("a download that was just added is findable by its hash");
    assert_eq!(found.id, id);

    t.remove(id, true).await.expect("remove");
    assert!(
        t.find_by_hash("045e85f2ebc24a875a64fe2e9ac9b61f7aad0499")
            .await
            .expect("lookup")
            .is_none(),
        "and gone once removed"
    );
}
