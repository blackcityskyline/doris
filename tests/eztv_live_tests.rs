//! Live-network checks for the EZTV source -- ignored by default so
//! `cargo test` stays offline-safe. Run manually with:
//! `cargo test --test eztv_live_tests -- --ignored --nocapture`

use doris::sources::eztv::EztvSearcher;
use doris::sources::source::{SearchRequest, Source};

#[tokio::test]
#[ignore = "requires network access to eztvx.to"]
async fn live_browse_returns_the_newest_releases_with_a_working_cursor() {
    let eztv = EztvSearcher::new();
    let first = eztv
        .search(&SearchRequest::new("", 0))
        .await
        .expect("live EZTV browse, page 1");

    println!(
        "page1 rows={}, has_more={}, next_offset={:?}",
        first.items.len(),
        first.has_more,
        first.next_offset
    );
    for row in first.items.iter().take(3) {
        println!(
            "  title={} size={} seeds={} date={} hash={}",
            row.title, row.size, row.seeds, row.date, row.info_hash
        );
    }

    assert!(!first.items.is_empty(), "live EZTV browse returned nothing");
    assert!(
        first.items.len() <= 100,
        "more rows than the requested limit: {}",
        first.items.len()
    );
    for row in &first.items {
        assert_eq!(row.info_hash.len(), 40, "hex40 only: {}", row.title);
        assert_eq!(row.group, Some(doris::sources::source::Group::TV));
        assert_eq!(row.download_url, "", "magnet-only rows");
        assert!(row.magnet.is_some());
    }

    // The cursor must actually walk the index: page 2 is a different
    let cursor = first.next_offset.expect("the page hands back its cursor");
    let second = eztv
        .search(&SearchRequest::new("", cursor))
        .await
        .expect("live EZTV browse, page 2");
    println!("page2 rows={} (cursor {})", second.items.len(), cursor);

    let first_hashes: Vec<&str> = first.items.iter().map(|r| r.info_hash.as_str()).collect();
    let second_hashes: Vec<&str> = second.items.iter().map(|r| r.info_hash.as_str()).collect();
    assert!(
        second_hashes.iter().all(|h| !first_hashes.contains(h)),
        "page 2 repeated page 1's rows -- the cursor did not move"
    );
}

#[tokio::test]
#[ignore = "requires network access to eztvx.to"]
async fn live_a_query_is_refused_with_a_reason_before_any_request() {
    let eztv = EztvSearcher::new();
    let err = eztv
        .search(&SearchRequest::new("breaking bad", 0))
        .await
        .expect_err("EZTV's API ignores `search`, so a query must not browse");

    let message = err.to_string();
    println!("refusal: {}", message);
    assert!(message.contains("no search"), "{}", message);
    assert!(message.contains("ignores"), "{}", message);
}
