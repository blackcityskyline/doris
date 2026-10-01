//! Live-network checks for the nnmclub source -- ignored by default so
//! `cargo test` stays offline-safe. Run manually with:
//! `cargo test --test nnmclub_live_tests -- --ignored --nocapture`
//!
//! Unlike wave 2, this host was reachable the whole time wave 3 was
//! written (200 to a browser UA, 0.3 s per page, no challenge), so the
//! assertions below are the ones the code was *written* from: the
//! search page's row shape, the `start=` cursor being disjoint across
//! pages, browse answering the empty query, a miss answering an empty
//! page, and `download.php?id=` handing over a bencoded file rather
//! than a block page. They still skip on an unreachable host for the
//! same reason `nyaa_live_tests` does: a network problem is not
//! evidence about the parser.

use doris::sources::nnmclub::{search_url, NnmclubSearcher, PAGE_SIZE};
use doris::sources::source::{Group, SearchRequest, Source};

/// The tracker answering at all, or `None` when this network is the
/// thing standing in the way (the caller then skips).
async fn require_host() -> Option<()> {
    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36",
        )
        .build()
        .ok()?;
    let url = search_url("frieren", 0, None);
    match client.get(&url).send().await {
        Ok(response) if response.status().is_success() => Some(()),
        Ok(response) => {
            println!(
                "SKIP: nnmclub answered HTTP {} for {} -- nothing here says \
                 anything about the parser",
                response.status(),
                url
            );
            None
        }
        Err(err) => {
            println!("SKIP: nnmclub is unreachable from this network: {}", err);
            None
        }
    }
}

#[tokio::test]
#[ignore = "requires network access to nnmclub.to"]
async fn live_rows_carry_a_link_to_a_torrent_and_a_page_to_read() {
    if require_host().await.is_none() {
        return;
    }
    let nnm = NnmclubSearcher::new();
    let page = nnm
        .search(&SearchRequest::new("frieren", 0))
        .await
        .expect("live nnmclub search");
    println!(
        "rows={}, has_more={}, next_offset={:?}",
        page.items.len(),
        page.has_more,
        page.next_offset
    );
    assert!(
        !page.items.is_empty(),
        "live nnmclub search returned nothing"
    );

    for row in &page.items {
        assert_eq!(row.source, "nnmclub");
        assert!(
            row.download_url
                .starts_with("https://nnmclub.to/forum/download.php?id="),
            "the row's own torrent link: {}",
            row.download_url
        );
        assert!(
            row.page_url
                .starts_with("https://nnmclub.to/forum/viewtopic.php?t="),
            "the topic the title came from: {}",
            row.page_url
        );
        // Wave-3 decision: no fan-out, so nothing to show for these.
        assert_eq!(row.magnet, None);
        assert_eq!(row.info_hash, "");
        // B6: a row claims the group of its own forum -- or nothing,
        // for sections outside the four groups (3D, fonts and books
        // came back in this very query, live).
        if let Some(group) = row.group {
            assert!(
                nnm.groups().contains(&group),
                "{:?} is not a group nnmclub declares",
                group
            );
        }
        assert!(!row.title.is_empty(), "a row nobody can recognise");
        assert!(row.size_bytes > 0, "{} has no size", row.title);
        assert!(row.added > 0, "{} has no date", row.title);
    }

    // The attribution working on real rows, not just on fixtures:
    // "frieren" comes back from the anime forums, and those claim
    // their group (26.09.2026: sections 169/621/626/632/644).
    assert!(
        page.items.iter().any(|row| row.group.is_some()),
        "not one row could be attributed to its forum -- the forum \
         cell is gone or the table is stale"
    );

    // A narrow query is a short page, and a short page is the last one
    // -- that is what makes `has_more == false` honest rather than a
    // missing feature.
    assert!(
        !page.has_more,
        "{} rows cannot be a full page",
        page.items.len()
    );
    assert_eq!(page.next_offset, None);
}

#[tokio::test]
#[ignore = "requires network access to nnmclub.to"]
async fn live_a_full_page_promises_the_next_and_the_next_is_disjoint() {
    if require_host().await.is_none() {
        return;
    }
    let nnm = NnmclubSearcher::new();
    let first = nnm
        .search(&SearchRequest::new("the", 0))
        .await
        .expect("live nnmclub page 1");
    println!(
        "page1 rows={}, has_more={}, next_offset={:?}",
        first.items.len(),
        first.has_more,
        first.next_offset
    );
    assert_eq!(
        first.items.len(),
        PAGE_SIZE,
        "a broad query fills the site's page"
    );
    assert!(first.has_more, "a full page promises another");
    assert_eq!(first.next_offset, Some(PAGE_SIZE), "cursor in page units");

    let offset = first.next_offset.expect("the cursor above");
    let second = nnm
        .search(&SearchRequest::new("the", offset))
        .await
        .expect("live nnmclub page 2");
    println!("page2 rows={}", second.items.len());
    assert!(!second.items.is_empty(), "page 2 came back empty");

    // The claim that licenses the cursor: no topic appears twice.
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
#[ignore = "requires network access to nnmclub.to"]
async fn live_an_empty_query_reads_the_freshest_topics() {
    if require_host().await.is_none() {
        return;
    }
    let nnm = NnmclubSearcher::new();
    assert!(nnm.supports_browse(), "and this is what it is backed by");

    let page = nnm
        .search(&SearchRequest::new("", 0))
        .await
        .expect("live nnmclub browse");
    println!(
        "browse rows={}, has_more={}, next_offset={:?}",
        page.items.len(),
        page.has_more,
        page.next_offset
    );
    assert!(
        !page.items.is_empty(),
        "the browse URL the code builds returned nothing"
    );
    assert!(
        page.items.len() <= PAGE_SIZE,
        "more rows than the site puts on a page: {}",
        page.items.len()
    );
}

#[tokio::test]
#[ignore = "requires network access to nnmclub.to"]
async fn live_a_query_with_no_matches_comes_back_empty_and_not_failed() {
    if require_host().await.is_none() {
        return;
    }
    let nnm = NnmclubSearcher::new();
    let page = nnm
        .search(&SearchRequest::new("zzqqxxnothing123", 0))
        .await
        .expect("a miss answers 200 with `Не найдено`, live");
    assert!(
        page.items.is_empty(),
        "{} rows for a string nobody indexed",
        page.items.len()
    );
    assert!(!page.has_more);
}

#[tokio::test]
#[ignore = "requires network access to nnmclub.to"]
async fn live_the_torrent_a_row_ships_is_a_bencoded_file() {
    if require_host().await.is_none() {
        return;
    }
    let nnm = NnmclubSearcher::new();
    let page = nnm
        .search(&SearchRequest::new("frieren", 0))
        .await
        .expect("live nnmclub search");
    let url = page
        .items
        .first()
        .expect("at least one row")
        .download_url
        .clone();

    let bytes = nnm
        .download_torrent(&url)
        .await
        .expect("the row's own link answers");
    println!("GET {} -> {} bytes", url, bytes.len());
    // What `spawn_stream` and the `d` key will do with it: hand these
    // bytes to TorrServer. A challenge page in their place would be
    // accepted silently -- hence the check on the wire, not in a test.
    assert!(!bytes.is_empty(), "an empty file would reach TorrServer");
    assert_ne!(bytes.first(), Some(&b'<'), "an HTML page is not a torrent");
    assert_eq!(
        bytes.first(),
        Some(&b'd'),
        "a torrent file is a bencoded dict, live-verified"
    );
}

///  live claim: a selected category narrows what the *server*
/// answers, and every row that comes back claims that category from
/// its own forum cell. Movies is the biggest list (80 `f%5B%5D=` ids,
/// ~1 KB of URL), so it is the one that proves one request is enough;
/// Anime is the small one that proves the attribution is not a
/// coincidence of a single group. If the tracker ignored the params,
/// rows from music/books/programs would come back claiming nothing and
/// both loops below would fail.
#[tokio::test]
#[ignore = "requires network access to nnmclub.to"]
async fn live_a_selected_category_answers_with_only_that_category() {
    if require_host().await.is_none() {
        return;
    }
    let nnm = NnmclubSearcher::new();

    for (query, category) in [("2026", Group::Movies), ("frieren", Group::Anime)] {
        let mut req = SearchRequest::new(query, 0);
        req.category = Some(category);
        let page = nnm
            .search(&req)
            .await
            .unwrap_or_else(|err| panic!("{:?} search failed: {}", category, err));
        println!("{:?} ({}) -> {} rows", category, query, page.items.len());
        assert!(
            !page.items.is_empty(),
            "{:?} asked for its own forums and got nothing",
            category
        );
        for row in &page.items {
            assert_eq!(
                row.group,
                Some(category),
                "{} came back from a forum outside {:?}",
                row.title,
                category
            );
        }
        // The tracker rate-limits bursts (429 seen live), so the two
        // requests do not arrive back to back.
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
    }
}
