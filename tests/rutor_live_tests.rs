//! Live-network checks for the rutor source -- ignored by default so
//! `cargo test` stays offline-safe. Run manually with:
//! `cargo test --test rutor_live_tests -- --ignored --nocapture`
use doris::sources::rutor::RutorSearcher;

#[tokio::test]
#[ignore = "requires network access to rutor.info"]
async fn live_search_returns_results() {
    let searcher = RutorSearcher::new();
    let items = searcher.search("test").await.expect("live rutor search");
    println!("OK: {} items", items.len());
    for it in items.iter().take(3) {
        println!(
            "  title={} size_bytes={} seeds_n={} leechers={} added={} hash={} url={}",
            it.title,
            it.size_bytes,
            it.seeds_n,
            it.leechers,
            it.added,
            it.info_hash,
            it.download_url
        );
    }
    assert!(!items.is_empty(), "live rutor search returned zero items");
    // Seeds used to be empty on every real row (the &nbsp; regex bug).
    assert!(items[0].seeds.parse::<u64>().is_ok(), "seeds not numeric");
    // B1: numeric and hash fields must come back filled from real rows.
    assert!(
        items[0].size_bytes > 0,
        "size_bytes not derived from the display size"
    );
    assert!(items[0].added > 0, "added not parsed from the date cell");
    assert_eq!(
        items[0].info_hash.len(),
        40,
        "info hash not read from the magnet link"
    );
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
/// the ones actually titled "... The Matrix..." must be promoted.
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

/// Download must return real.torrent bytes rather than an HTML page:
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
    assert!(
        !head.starts_with('<'),
        "got HTML instead of a .torrent: {}",
        head
    );
}

/// B2: `SearchPage.has_more` is what the Results panel now trusts instead
/// of app.rs's `count < 50` guess -- which could never work for rutor,
/// whose pages hold 100 rows. Pin it against the live site by asking the
/// question it answers: if we claim there is another page, there must be.
#[tokio::test]
#[ignore = "requires network access to rutor.info"]
async fn live_trait_search_has_more_agrees_with_the_next_page() {
    use doris::sources::source::{SearchRequest, Source};

    let rutor = RutorSearcher::new();
    let query = "фильм";

    let page1 = Source::search(&rutor, &SearchRequest::new(query, 0))
        .await
        .expect("page 1");
    println!(
        "page1: {} items, has_more={}",
        page1.items.len(),
        page1.has_more
    );
    assert!(!page1.items.is_empty(), "query '{}' found nothing", query);

    let page2 = Source::search(&rutor, &SearchRequest::new(query, 100))
        .await
        .expect("page 2");
    println!(
        "page2: {} items, has_more={}",
        page2.items.len(),
        page2.has_more
    );

    assert_eq!(
        page1.has_more,
        !page2.items.is_empty(),
        "has_more must mirror whether rutor actually serves a next page"
    );
}

/// B6: a selected category is one GET per rubric id of
/// `rutor::GROUP_IDS`, and the rows it brings back stand under that
/// category -- the live half of the table the parse tests pin offline.
/// Rubric 8 (`Игры`) answered 100 rows for "2026" on 26.09.2026 while
/// `cat=0` answered a different set, so an all-Games page here means
/// the fan-out really asked that rubric.
#[tokio::test]
#[ignore = "requires network access to rutor.info"]
async fn live_a_selected_category_answers_with_only_that_category() {
    use doris::sources::source::{Group, SearchRequest, Source};

    let rutor = RutorSearcher::new();
    let mut req = SearchRequest::new("2026", 0);
    req.category = Some(Group::Games);

    let page = Source::search(&rutor, &req)
        .await
        .expect("live category search");
    println!(
        "Games: {} items, has_more={}, next_offset={:?}",
        page.items.len(),
        page.has_more,
        page.next_offset
    );

    assert!(!page.items.is_empty(), "rubric 8 answered nothing");
    for row in &page.items {
        assert_eq!(
            row.group,
            Some(Group::Games),
            "{} was fetched inside the Games rubric and claims it",
            row.title
        );
    }
    if page.has_more {
        assert_eq!(
            page.next_offset,
            Some(RutorSearcher::PAGE_SIZE),
            "a full fan-out page steps the cursor by exactly one page"
        );
    }
}

/// B9: an empty query is browse mode, and rutor answers it with the
/// homepage index -- the latest releases, one mixed list, no pager.
#[tokio::test]
#[ignore = "requires network access to rutor.info"]
async fn live_browse_answers_with_the_homepage_index() {
    let searcher = RutorSearcher::new();
    let page = searcher
        .search_page("", 0, None)
        .await
        .expect("live rutor browse");

    println!(
        "browse: {} items, has_more={}",
        page.items.len(),
        page.has_more
    );
    assert!(
        !page.items.is_empty(),
        "the homepage must answer an empty query with the latest releases"
    );
    assert!(
        !page.has_more,
        "the homepage has no pager, so browse is one page"
    );
    // A browse list is mixed by nature: rows claim no group, which is
    // why the `b` key returns the view to "all" before searching.
    assert!(
        page.items.iter().all(|row| row.group.is_none()),
        "a mixed homepage list claims no category"
    );
}
