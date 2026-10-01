//! Live-network checks for the SubsPlease source -- ignored by default
//! so `cargo test` stays offline-safe. Run manually with:
//! `cargo test --test subsplease_live_tests -- --ignored --nocapture`

use doris::sources::source::{SearchRequest, Source};
use doris::sources::subsplease::SubsPleaseSearcher;

#[tokio::test]
#[ignore = "requires network access to subsplease.org"]
async fn live_a_query_returns_one_best_resolution_row_per_release() {
    let sp = SubsPleaseSearcher::new();
    let page = sp
        .search(&SearchRequest::new("frieren", 0))
        .await
        .expect("live SubsPlease search");

    println!(
        "rows={}, has_more={}, next_offset={:?}",
        page.items.len(),
        page.has_more,
        page.next_offset
    );
    for row in page.items.iter().take(4) {
        println!(
            "  title={} size={} date={} hash={}",
            row.title, row.size, row.date, row.info_hash
        );
    }

    assert!(
        !page.items.is_empty(),
        "live SubsPlease search returned nothing"
    );
    assert!(!page.has_more, "this API has no cursor to offer");
    assert_eq!(page.next_offset, None);
    for row in &page.items {
        assert_eq!(
            row.info_hash.len(),
            40,
            "base32 must have become hex: {}",
            row.title
        );
        assert!(
            row.info_hash.chars().all(|c| c.is_ascii_hexdigit()),
            "{}",
            row.info_hash
        );
        assert_eq!(row.download_url, "", "magnet-only rows");
        assert!(row.magnet.is_some());
        assert_eq!(row.group, Some(doris::sources::source::Group::Anime));
    }
    // No episode may occupy two rows (480/720/1080 collapse).
    let mut titles: Vec<&str> = page.items.iter().map(|r| r.title.as_str()).collect();
    titles.sort_unstable();
    let unique = titles.clone();
    titles.dedup();
    assert_eq!(titles.len(), unique.len(), "an episode appeared twice");
}

#[tokio::test]
#[ignore = "requires network access to subsplease.org"]
async fn live_browse_returns_the_latest_releases() {
    let sp = SubsPleaseSearcher::new();
    let page = sp
        .search(&SearchRequest::new("", 0))
        .await
        .expect("live SubsPlease browse");

    println!("browse rows={}", page.items.len());
    assert!(
        !page.items.is_empty(),
        "an empty query must browse, not stall"
    );
    assert!(
        page.items.len() <= 100,
        "one row per release, not one per quality"
    );

    // `f=latest` is the point of the browse path: what comes back is
    let now = chrono::Utc::now().timestamp();
    let oldest = page
        .items
        .iter()
        .map(|r| r.added)
        .filter(|&added| added > 0)
        .min()
        .expect("latest releases must carry dates");
    assert!(
        oldest >= now - 14 * 86_400,
        "the oldest \"latest\" row is {} days old",
        (now - oldest) / 86_400
    );
}
