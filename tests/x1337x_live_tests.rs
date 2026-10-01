//! Live-network checks for the 1337x source -- ignored by default so
//! `cargo test` stays offline-safe. Run manually with:
//! `cargo test --test x1337x_live_tests -- --ignored --nocapture`
//!
//! These are the checks the source was written against on 25.09.2026,
//! kept alive as the alarm that something moved: the mirror order (three
//! of torio's four hosts answer a Cloudflare challenge, one answers
//! everything), a query's shape through the client-side filter and its
//! fallback, the page cursor staying on the site's grid, browse reading
//! `/home/`, and -- the one that costs requests -- every browse row
//! coming back with a day, which only holds if the rows whose list cell
//! says `03:15am` really do find `Date uploaded` on their own page.
//!
//! They skip when the mirror cannot be reached, because an unreachable
//! host says nothing about the parser.

use doris::sources::source::{Group, SearchRequest, Source};
use doris::sources::x1337x::{search_url, X1337xSearcher, HOSTS, PAGE_SIZE};

/// The user agent the probes ran with.
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36";

/// The mirror answering at all, or `None` when this network is the
/// thing standing in the way (the caller then skips).
async fn require_host() -> Option<()> {
    let client = reqwest::Client::builder().user_agent(UA).build().ok()?;
    match client
        .get(search_url(HOSTS[0], "frieren", 0, None))
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => Some(()),
        Ok(response) => {
            println!(
                "SKIP: {} answered HTTP {} -- nothing here says anything about the parser",
                HOSTS[0],
                response.status()
            );
            None
        }
        Err(err) => {
            println!("SKIP: 1337x unreachable from this network: {}", err);
            None
        }
    }
}

#[tokio::test]
#[ignore = "requires network access to 1337x"]
async fn live_a_single_word_query_comes_back_precise_and_full() {
    if require_host().await.is_none() {
        return;
    }
    let source = X1337xSearcher::new();
    let page = source
        .search(&SearchRequest::new("frieren", 0))
        .await
        .expect("live 1337x search");
    println!(
        "rows={}, has_more={}, next_offset={:?}",
        page.items.len(),
        page.has_more,
        page.next_offset
    );

    assert_eq!(
        page.items.len(),
        PAGE_SIZE,
        "a broad single-word query fills the site's page"
    );
    assert!(page.has_more, "a full page promises another");
    assert_eq!(page.next_offset, Some(PAGE_SIZE), "cursor in page units");

    for row in &page.items {
        assert_eq!(row.source, "1337x");
        assert!(
            row.page_url.starts_with("https://"),
            "a row with nowhere to go: {}",
            row.page_url
        );
        // One word: taken exactly as the engine answered it, metadata
        // matches included -- that is the decision the filter encodes.
        assert!(
            row.title.to_lowercase().contains("frieren"),
            "one word, 20 of 20 live -- this is one of the exceptions: {}",
            row.title
        );
        assert!(!row.title.is_empty());
        assert!(row.size_bytes > 0, "{} has no size", row.title);
        assert!(row.added > 0, "{} has no date", row.title);
        // Lazy magnet (decision): the row arrives playable-looking but
        // link-less, and `resolve_magnet` is the way to the link.
        assert_eq!(row.magnet, None);
        assert_eq!(row.download_url, "");
        assert_eq!(row.info_hash, "");
        assert_eq!(row.group, None, "groups are declared, not guessed");
    }
}

/// slot as the user meets it: a category selected in the row is
/// what the URL says out loud, and every row it brings back stands
/// under that category. Live on 26.09.2026 the site's own `/sub/`
/// links agreed with the path 20 of 20 rows for each label probed;
/// this is the same claim read back from the source.
#[tokio::test]
#[ignore = "requires network access to 1337x"]
async fn live_a_selected_category_is_what_the_path_and_the_rows_say() {
    if require_host().await.is_none() {
        return;
    }
    let source = X1337xSearcher::new();
    let mut req = SearchRequest::new("matrix", 0);
    req.category = Some(Group::Movies);
    let page = source
        .search(&req)
        .await
        .expect("live 1337x category search");
    println!(
        "Movies rows={}, has_more={}",
        page.items.len(),
        page.has_more
    );
    assert!(!page.items.is_empty(), "the category path answered nothing");
    for row in &page.items {
        assert_eq!(
            row.group,
            Some(Group::Movies),
            "{} arrived inside the Movies path and claims it",
            row.title
        );
    }
}

#[tokio::test]
#[ignore = "requires network access to 1337x"]
async fn live_a_page_nothing_answers_is_empty_and_still_pages() {
    if require_host().await.is_none() {
        return;
    }
    let source = X1337xSearcher::new();
    let page = source
        .search(&SearchRequest::new("frieren crack", 0))
        .await
        .expect("live 1337x search");
    println!(
        "rows={}, has_more={}, next_offset={:?}",
        page.items.len(),
        page.has_more,
        page.next_offset
    );

    // The engine ORs this query, and on 25.09.2026 not one of its
    // first three pages held a row with *both* words in the title.
    // Nothing matching means an empty table: the raw-page fallback that
    // used to guarantee a row is exactly what put unrelated torrents in
    // front of the user (a "Games" search full of repacks the query
    // never mentioned). What the page must still carry is the way out --
    // the cursor on the server's own full page, so the TUI's
    // `needs_more` can ask for page 2 from a table with no rows in it.
    assert!(
        page.items.is_empty(),
        "no row answered, so no row is shown: {:#?}",
        page.items
    );
    assert!(page.has_more, "the server's page was full");
    assert_eq!(
        page.next_offset,
        Some(PAGE_SIZE),
        "cursor from the server's full page, not from the survivors"
    );
}

#[tokio::test]
#[ignore = "requires network access to 1337x"]
async fn live_a_full_page_promises_the_next_and_the_next_is_disjoint() {
    if require_host().await.is_none() {
        return;
    }
    let source = X1337xSearcher::new();
    let first = source
        .search(&SearchRequest::new("dune", 0))
        .await
        .expect("live 1337x page 1");
    println!(
        "page1 rows={}, has_more={}, next_offset={:?}",
        first.items.len(),
        first.has_more,
        first.next_offset
    );
    assert_eq!(first.items.len(), PAGE_SIZE, "page 1 is a full page");
    assert!(first.has_more);
    assert_eq!(first.next_offset, Some(PAGE_SIZE));

    let offset = first.next_offset.expect("the cursor above");
    let second = source
        .search(&SearchRequest::new("dune", offset))
        .await
        .expect("live 1337x page 2");
    println!("page2 rows={}", second.items.len());
    assert!(!second.items.is_empty(), "page 2 came back empty");

    // The claim that licenses the cursor: `/search/<q>/<N>/` counts
    // pages of rows, so no torrent path appears twice.
    let page_one: Vec<&str> = first
        .items
        .iter()
        .map(|row| row.page_url.as_str())
        .collect();
    for row in &second.items {
        assert!(
            !page_one.contains(&row.page_url.as_str()),
            "{} repeated across pages -- the cursor would loop",
            row.page_url
        );
    }
}

#[tokio::test]
#[ignore = "requires network access to 1337x"]
async fn live_browse_reads_the_freshest_rows_and_dates_every_one() {
    if require_host().await.is_none() {
        return;
    }
    let source = X1337xSearcher::new();
    assert!(source.supports_browse(), "and this is what backs it");

    let page = source
        .search(&SearchRequest::new("", 0))
        .await
        .expect("live 1337x browse");
    println!(
        "browse rows={}, has_more={}, next_offset={:?}",
        page.items.len(),
        page.has_more,
        page.next_offset
    );
    assert!(
        page.items.len() > PAGE_SIZE,
        "the site's front page answered {} rows",
        page.items.len()
    );
    // `/home/` has no pager, and the browse path must not invent one.
    assert!(!page.has_more, "browse never promises a second fetch");
    assert_eq!(page.next_offset, None);

    // The claim that costs requests: 11 of these 78 rows show `03:15am`
    // in the list, and every one of them gets its day from its own
    // page. If this ever fails, the date column has started lying
    // about the freshest rows -- or the detail parser moved.
    let undated: Vec<&str> = page
        .items
        .iter()
        .filter(|row| row.added == 0 || row.date.is_empty())
        .map(|row| row.title.as_str())
        .collect();
    assert!(
        undated.is_empty(),
        "rows the list dated as a time and no detail page corrected: {:?}",
        undated
    );
}

#[tokio::test]
#[ignore = "requires network access to 1337x"]
async fn live_a_query_with_no_matches_comes_back_empty_and_not_failed() {
    if require_host().await.is_none() {
        return;
    }
    let source = X1337xSearcher::new();
    let page = source
        .search(&SearchRequest::new("zzqqxxnothing123", 0))
        .await
        .expect("a miss answers 200 with the table header, live");
    assert!(
        page.items.is_empty(),
        "{} rows for a string nobody indexed",
        page.items.len()
    );
    assert!(!page.has_more, "and promises no second page of nothing");
    assert_eq!(page.next_offset, None);
}

#[tokio::test]
#[ignore = "requires network access to 1337x"]
async fn live_the_rows_own_page_carries_the_playable_magnet() {
    if require_host().await.is_none() {
        return;
    }
    let source = X1337xSearcher::new();
    let page = source
        .search(&SearchRequest::new("dune", 0))
        .await
        .expect("live 1337x search");
    let row = &page.items[0];
    println!("resolving {}", row.page_url);

    let magnet = source
        .resolve_magnet(&row.page_url)
        .await
        .expect("the row's page answered")
        .expect("and it carries a magnet -- the site serves no file");
    assert!(
        magnet.starts_with("magnet:?xt=urn:btih:"),
        "not a magnet: {}",
        magnet
    );
    let hash = &magnet["magnet:?xt=urn:btih:".len()..];
    let hash: String = hash.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    assert_eq!(
        hash.len(),
        40,
        "a v1 info hash, got {} in {}",
        hash.len(),
        magnet
    );
}
