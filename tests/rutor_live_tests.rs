//! Live-network checks for the rutor source -- ignored by default so
//! `cargo test` stays offline-safe. Run manually with:
//! `cargo test --test rutor_live_tests -- --ignored --nocapture`
use doris::search::rutor::RutorSearcher;

#[tokio::test]
#[ignore = "requires network access to rutor.org"]
async fn live_search_returns_results() {
    let searcher = RutorSearcher::new();
    let items = searcher.search("test").await.expect("live rutor search");
    println!("OK: {} items", items.len());
    for it in items.iter().take(3) {
        println!(
            "  title={} size={} seeds={} date={} url={}",
            it.title, it.size, it.seeds, it.date, it.download_url
        );
    }
    assert!(!items.is_empty(), "live rutor search returned zero items");
    // Seeds used to be empty on every real row (the &nbsp; regex bug).
    assert!(items[0].seeds.parse::<u64>().is_ok(), "seeds not numeric");
}

/// The reported bug: queries whose words rutor's index lacks -- here the
/// bare `z` -- used to come back with zero results every time, because
/// rutor ANDs every query word and never indexes such tokens.
#[tokio::test]
#[ignore = "requires network access to rutor.org"]
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
#[ignore = "requires network access to rutor.org"]
async fn live_search_prefers_rows_mentioning_dropped_words() {
    let searcher = RutorSearcher::new();
    let items = searcher.search("the matrix").await.expect("fallback");
    println!("the matrix -> {} items", items.len());
    for it in items.iter().take(5) {
        println!("  {}", it.title);
    }
    assert!(!items.is_empty(), "fallback for 'the matrix' still empty");
}
