//! Live end-to-end check for B7's add-by-link -- ignored by default so
//! `cargo test` stays offline-safe. Run manually with:
//! `cargo test --test torrserver_live_tests -- --ignored --nocapture`
//!
//! What it exercises, in order: a *real* magnet taken from a *real* rutor
//! row (exactly the input the streaming path hands over), the 200 + hash
//! that `add_by_link` reads, TorrServer actually knowing the torrent,
//! and `rem` forgetting it again -- so the check leaves the server the
//! way it found it.

use doris::sources::rutor::RutorSearcher;
use doris::torrserver::api::TorrServer;

const TORRSERVER: &str = "http://localhost:8090";

#[tokio::test]
#[ignore = "needs rutor.info, a local TorrServer, and permission to add one torrent"]
async fn live_add_by_link_adds_lists_and_forgets_a_real_magnet() {
    let torrserver = TorrServer::new(TORRSERVER);
    assert!(
        torrserver.is_reachable().await,
        "no TorrServer answering on {}: start it first",
        TORRSERVER
    );

    let searcher = RutorSearcher::new();
    // rutor ANDs every query word, so a multi-word phrase can legitimately
    // return nothing -- this test is about the link path, not about search
    // recall, hence a word the site certainly indexes.
    let items = searcher.search("matrix").await.expect("live rutor search");
    println!("rutor returned {} rows", items.len());
    assert!(!items.is_empty(), "rutor search itself came back empty");
    let row = items
        .iter()
        .find(|item| item.magnet.is_some())
        .expect("rutor.info rows carry inline magnets");
    let magnet = row.magnet.as_deref().expect("just checked");
    println!("title: {}", row.title);
    println!("magnet: {}", magnet);

    let hash = torrserver
        .add_by_link(magnet, &row.title)
        .await
        .expect("TorrServer accepted the link");
    println!("added hash: {}", hash);
    assert_eq!(hash.len(), 40, "a bittorrent hash is 40 hex chars");
    assert!(
        hash.chars().all(|c| c.is_ascii_hexdigit()),
        "expected hex, got {}",
        hash
    );
    // The hash is what the magnet carries, so the server parsed the link
    // we sent rather than inventing an entry of its own.
    let from_link = magnet
        .split("xt=urn:btih:")
        .nth(1)
        .and_then(|rest| rest.split(['&', '?']).next())
        .unwrap_or_default();
    assert_eq!(hash, from_link.to_lowercase(), "hash must come from the link");

    let listed = torrserver
        .get_torrent(&hash)
        .await
        .expect("get must not error");
    assert!(listed.is_some(), "the added torrent must be listable");
    println!(
        "listed: name={:?} hash={:?}",
        listed.as_ref().map(|t| t.name.clone()),
        listed.as_ref().map(|t| t.hash.clone())
    );

    torrserver.remove(&hash).await.expect("remove must succeed");
    let after = torrserver
        .get_torrent(&hash)
        .await
        .expect("get after remove must not error");
    assert!(after.is_none(), "rem must have forgotten the torrent");
    println!("cleaned up");
}

/// The regression the B7 verification exposed: `TorrentInfo` only knew
/// the capitalized Go names, while a modern TorrServer answers with
/// json-tagged ones -- and `#[serde(default)]` turned that mismatch into
/// *empty* fields instead of an error, leaving the Torrent zone without
/// a hash, name, size or speed. Ignored like the rest, because it needs
/// a running server; it does not touch it (read-only `list`).
#[tokio::test]
#[ignore = "needs a local TorrServer"]
async fn live_torrent_list_parses_against_the_running_server() {
    let torrserver = TorrServer::new(TORRSERVER);
    assert!(
        torrserver.is_reachable().await,
        "no TorrServer answering on {}: start it first",
        TORRSERVER
    );

    let list = torrserver.list_torrents().await.expect("list must not error");
    println!("TorrServer lists {} torrents", list.len());
    if let Some(first) = list.first() {
        println!(
            "first: name={:?} hash={:?} size={} status={:?}",
            first.name, first.hash, first.total_size, first.status_string
        );
        assert!(
            !first.hash.is_empty(),
            "hash came back empty -- the response keys are not the ones we rename to"
        );
        assert!(!first.name.is_empty(), "title came back empty");
        assert!(first.total_size > 0, "torrent_size came back empty");
        assert!(!first.status_string.is_empty(), "stat_string came back empty");
    } else {
        println!("no torrents to inspect (empty list is a valid answer)");
    }
}
