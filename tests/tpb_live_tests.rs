//! Live-network checks for the TPB/apibay source -- ignored by default
//! so `cargo test` stays offline-safe. Run manually with:
//! `cargo test --test tpb_live_tests -- --ignored --nocapture`

use doris::sources::source::{SearchRequest, Source};
use doris::sources::tpb::TpbSearcher;

#[tokio::test]
#[ignore = "requires network access to apibay.org"]
async fn live_a_query_returns_at_most_a_hundred_rows_and_never_more() {
    let tpb = TpbSearcher::new();
    let page = tpb
        .search(&SearchRequest::new("matrix", 0))
        .await
        .expect("live apibay search");

    println!(
        "rows={}, has_more={}, next_offset={:?}",
        page.items.len(),
        page.has_more,
        page.next_offset
    );
    for row in page.items.iter().take(3) {
        println!(
            "  title={} size={} seeds={} added={} group={:?}",
            row.title, row.size, row.seeds, row.date, row.group
        );
    }

    assert!(
        !page.items.is_empty(),
        "live apibay search returned nothing"
    );
    // apibay tops out at 100 and ignores page=, so a page promising
    assert!(!page.has_more, "apibay has no cursor to offer");
    assert_eq!(page.next_offset, None);
    assert!(
        page.items.len() <= 100,
        "more rows than apibay's cap: {}",
        page.items.len()
    );
    assert!(
        page.items.iter().all(|r| !r.title.contains("No results")),
        "the placeholder row leaked into the table"
    );
    assert!(
        page.items.iter().all(|r| r.info_hash.len() == 40),
        "every real row carries a hash"
    );
}

#[tokio::test]
#[ignore = "requires network access to apibay.org"]
async fn live_browse_reads_both_top100_lists() {
    let tpb = TpbSearcher::new();
    let page = tpb
        .search(&SearchRequest::new("", 0))
        .await
        .expect("live apibay browse");

    let movies = page.items.iter().filter(|r| r.group.is_some()).count();
    println!(
        "browse rows={} (attributed={}), has_more={}",
        page.items.len(),
        movies,
        page.has_more
    );

    // Both lists are fetched, so the answer spans more than one list's
    assert!(
        page.items.len() > 100,
        "expected movies + episodes, got {}",
        page.items.len()
    );
    assert!(page
        .items
        .iter()
        .any(|r| r.group == Some(doris::sources::source::Group::Movies)));
    assert!(page
        .items
        .iter()
        .any(|r| r.group == Some(doris::sources::source::Group::TV)));
    assert!(!page.has_more, "the top-100 lists are fixed");
}

/// The category half of the apibay `cat=` filter: every row on the
/// answer has to come from the categories the group stands for, or the
/// tab above the table is claiming something the rows do not.
#[tokio::test]
#[ignore = "requires network access to apibay.org"]
async fn live_a_selected_category_answers_with_only_that_category() {
    use doris::sources::source::Group;

    let tpb = TpbSearcher::new();
    let mut req = SearchRequest::new("matrix", 0);
    req.category = Some(Group::Movies);
    let page = tpb.search(&req).await.expect("live apibay category search");

    println!(
        "Movies rows={}, has_more={}",
        page.items.len(),
        page.has_more
    );
    assert!(!page.items.is_empty(), "cat= answered nothing");
    for row in &page.items {
        assert_eq!(
            row.group,
            Some(Group::Movies),
            "{} was fetched inside the Movies categories and claims it",
            row.title
        );
    }
}
