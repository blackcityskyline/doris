//! Live-network checks for the torentino source -- ignored by default so
//! `cargo test` stays offline-safe. Run manually with:
//! `cargo test --test torentino_live_tests -- --ignored --nocapture`
//!
//! What these pin against the real host (probed 26.09.2026): the POST
//! search answers rows, and a row's item page hands over a real
//! `.torrent` through the two-step download.

use doris::search::source::Group;
use doris::search::torentino::TorentinoSearcher;

#[tokio::test]
#[ignore = "requires network access to torentino.org"]
async fn live_search_answers_rows_claiming_games() {
    let searcher = TorentinoSearcher::new();
    let page = searcher
        .search_page("gta", 0)
        .await
        .expect("live torentino search");

    println!(
        "search: {} items, has_more={}",
        page.items.len(),
        page.has_more
    );
    assert!(!page.items.is_empty(), "live torentino search returned zero items");
    for row in &page.items {
        assert_eq!(
            row.group,
            Some(Group::Games),
            "{} must claim Games",
            row.title
        );
        assert!(
            !row.download_url.is_empty(),
            "{} must point at its item page",
            row.title
        );
    }
}

#[tokio::test]
#[ignore = "requires network access to torentino.org"]
async fn live_an_empty_query_is_refused_with_the_reason() {
    let searcher = TorentinoSearcher::new();
    let outcome = searcher.search_page("", 0).await;
    let message = outcome.expect_err("an empty query must be refused").to_string();
    println!("refused: {}", message);
    assert!(
        message.contains("needs terms"),
        "the refusal must say why, not just fail: {}",
        message
    );
}

#[tokio::test]
#[ignore = "requires network access to torentino.org"]
async fn live_download_follows_the_item_page_to_a_torrent() {
    let searcher = TorentinoSearcher::new();
    let page = searcher
        .search_page("gta", 0)
        .await
        .expect("live torentino search");

    // Some rows are placeholders ("ИГРА ПОКА НЕ ВЫШЛА" -- the game is
    // not out yet), so the first row with a real file is the one to
    // download; the refusal itself is part of what is being pinned.
    let mut downloaded = None;
    for row in page.items.iter().take(10) {
        match searcher.download_torrent(&row.download_url).await {
            Ok(bytes) => {
                downloaded = Some(bytes);
                break;
            }
            Err(e) => println!("  placeholder: {}: {}", row.title, e),
        }
    }

    let bytes = downloaded.expect("at least one row downloads a real .torrent");
    println!("download: {} bytes", bytes.len());
    assert!(bytes.len() > 1000, "a real .torrent is far bigger than that");
    // A bencoded torrent starts with "d" (a dict) -- the same check
    // `download_torrent`'s status guard implies, read off the bytes.
    assert_eq!(bytes[0], b'd', "the download is a bencoded torrent");
}

/// The placeholder case is refused with the reason, not uploaded: a
/// `.txt` reading "ИГРА ПОКА НЕ ВЫШЛА" must not reach TorrServer as if
/// it were a torrent.
#[tokio::test]
#[ignore = "requires network access to torentino.org"]
async fn live_a_placeholder_file_is_refused_with_the_reason() {
    let searcher = TorentinoSearcher::new();
    let page = searcher
        .search_page("gta 6", 0)
        .await
        .expect("live torentino search");

    // GTA 6 is unreleased, so its rows are the placeholder shape. Two
    // honest refusals exist and either is correct: the item page carries
    // no file link at all, or the link resolves to the "ИГРА ПОКА НЕ
    // ВЫШЛА" placeholder. What must never happen is the bytes coming
    // back as if they were a torrent.
    for row in &page.items {
        let outcome = searcher.download_torrent(&row.download_url).await;
        let message = outcome.expect_err("a placeholder must be refused").to_string();
        println!("  refused: {}: {}", row.title, message);
        assert!(
            message.contains("no .torrent link") || message.contains("not a .torrent"),
            "the refusal must say why: {}",
            message
        );
    }
}
