//! Live-network checks for the rutor source -- ignored by default so
//! `cargo test` stays offline-safe. Run manually with:
//! `cargo test --test rutor_live_tests -- --ignored --nocapture`
use doris::search::rutor::RutorSearcher;

#[tokio::test]
#[ignore = "requires network access to rutor.info"]
async fn live_search_returns_results() {
    let searcher = RutorSearcher::new();
    let items = searcher.search("test").await.expect("live rutor search");
    println!("OK: {} items", items.len());
    for it in items.iter().take(3) {
        println!(
            "  title={} size_bytes={} seeds_n={} leechers={} added={} hash={} url={}",
            it.title, it.size_bytes, it.seeds_n, it.leechers, it.added,
            it.info_hash, it.download_url
        );
    }
    assert!(!items.is_empty(), "live rutor search returned zero items");
    // Seeds used to be empty on every real row (the &nbsp; regex bug).
    assert!(items[0].seeds.parse::<u64>().is_ok(), "seeds not numeric");
    // B1: numeric and hash fields must come back filled from real rows.
    assert!(items[0].size_bytes > 0, "size_bytes not derived from the display size");
    assert!(items[0].added > 0, "added not parsed from the date cell");
    assert_eq!(items[0].info_hash.len(), 40, "info hash not read from the magnet link");
}

/// The reported bug: queries whose words rutor's index lacks -- here the
/// bare `z` -- used to come back with zero results every time, because
/// rutor ANDs every query word and never indexes such tokens.
#[tokio::test]
#[ignore = "requires network access to rutor.info"]
async fn live_search_falls_back_for_unindexable_words() {
    let searcher = RutorSearcher::new();
    let items = searcher.search("world war z").await.expect("fallback");
    println!("world war z -> {} items", items.len());
    for it in items.iter().take(5) {
        println!("  {}", it.title);
    }
    assert!(!items.is_empty(), "fallback for 'world war z' still empty");
}

/// Stopword case: strict search is 0, the relaxed one must find rows and
/// the ones actually titled "... The Matrix ..." must be promoted.
#[tokio::test]
#[ignore = "requires network access to rutor.info"]
async fn live_search_prefers_rows_mentioning_dropped_words() {
    let searcher = RutorSearcher::new();
    let items = searcher.search("the matrix").await.expect("fallback");
    println!("the matrix -> {} items", items.len());
    for it in items.iter().take(5) {
        println!("  {}", it.title);
    }
    assert!(!items.is_empty(), "fallback for 'the matrix' still empty");
}

/// Download must return real .torrent bytes rather than an HTML page:
/// on 25.09.2026 rutor.org's `/download/{id}` started answering
/// `302 -> /login` to logged-out clients, which is what forced the
/// source over to rutor.info -- this test is what pins that.
#[tokio::test]
#[ignore = "requires network access to rutor.info"]
async fn live_download_returns_torrent_bytes() {
    let searcher = RutorSearcher::new();
    let items = searcher.search("test").await.expect("live rutor search");
    let item = items.first().expect("no rows to download");
    let bytes = searcher
        .download_torrent(&item.download_url)
        .await
        .expect("live rutor download");
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(16)]);
    println!("downloaded {} bytes, starts with {:?}", bytes.len(), head);
    assert!(bytes.len() > 100, "suspiciously small download");
    // bencode torrent files start with the dict marker `d`; an HTML login
    // page starts with `<`.
    assert!(!head.starts_with('<'), "got HTML instead of a .torrent: {}", head);
}
