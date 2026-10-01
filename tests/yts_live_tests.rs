//! Live-network checks for the YTS source -- ignored by default so
//! `cargo test` stays offline-safe. Run manually with:
//! `cargo test --test yts_live_tests -- --ignored --nocapture`
//!
//! Hosts come from `yts::HOSTS`, so this doubles as the check that the
//! mirror list still has at least one live entry (it moved once already
//! -- see the module doc of `search/yts.rs`).

use doris::sources::source::{SearchRequest, Source};
use doris::sources::yts::{YtsSearcher, HOSTS};

#[tokio::test]
#[ignore = "requires network access to the YTS API"]
async fn live_a_query_returns_hashed_rows_with_sizes_and_hashes() {
    let yts = YtsSearcher::new();
    let page = yts
        .search(&SearchRequest::new("matrix", 0))
        .await
        .expect("live YTS search");

    println!(
        "{} rows, has_more={}, next_offset={:?}",
        page.items.len(),
        page.has_more,
        page.next_offset
    );
    for item in page.items.iter().take(3) {
        println!(
            "  title={} size={} seeds={} hash={} added={}",
            item.title, item.size, item.seeds, item.info_hash, item.added
        );
    }

    assert!(!page.items.is_empty(), "live YTS search returned nothing");
    let first = &page.items[0];
    assert_eq!(first.info_hash.len(), 40, "no hash on {:?}", first.title);
    assert!(
        first.info_hash.chars().all(|c| c.is_ascii_hexdigit()),
        "not hex: {}",
        first.info_hash
    );
    assert!(first.size_bytes > 0, "size_bytes not read from the API");
    assert!(first.magnet.is_some(), "every YTS row must be streamable");
    assert_eq!(first.download_url, "", "YTS has no .torrent to download");
}

#[tokio::test]
#[ignore = "requires network access to the YTS API"]
async fn live_browse_returns_the_newest_movies_first() {
    let yts = YtsSearcher::new();
    let page = yts
        .search(&SearchRequest::new("", 0))
        .await
        .expect("live YTS browse");

    println!("browse: {} rows", page.items.len());
    assert!(
        !page.items.is_empty(),
        "an empty query must browse, not stall"
    );

    // Not every YTS movie carries `date_uploaded_unix`, and what
    let now = chrono::Utc::now().timestamp();
    let dated: Vec<i64> = page
        .items
        .iter()
        .map(|i| i.added)
        .filter(|&d| d > 0)
        .collect();
    assert!(!dated.is_empty(), "a date-sorted list must have dates");
    let newest = dated.iter().copied().max().expect("dated is not empty");
    assert!(
        newest >= now - 7 * 86_400,
        "the freshest browse row is {} days old",
        (now - newest) / 86_400
    );
    println!("hosts in play: {:?}", HOSTS);
}
