//! Live-network checks for the nyaa source -- ignored by default so
//! `cargo test` stays offline-safe. Run manually with:
//! `cargo test --test nyaa_live_tests -- --ignored --nocapture`
//!
//! nyaa.si sits behind ddos-guard, which answered 504 to *every* path
//! from this network on 25.09.2026 while wave 2 was being written (the
//! feed itself was captured once, through a different route). So each
//! check below starts with a preflight and **skips, loudly**, when the
//! host will not answer: a blocked network says nothing about the
//! parser, and a red test everybody learns to ignore is worse than an
//! honest skip (the wave-2 decision, same as eztv's refusal).
//!
//! The assertions below are therefore the *unverified* claims -- paging,
//! browse and the `.torrent` links -- waiting for a network that lets
//! them through; everything else is already locked down offline in
//! `nyaa_parse_tests.rs`.

use doris::search::models::TorrentItem;
use doris::search::nyaa::{NyaaSearcher, feed_url, parse_items};
use doris::search::source::{Group, SearchRequest, Source};

const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36";

/// The live feed when nyaa lets this network through, `None` when it
/// does not -- in which case the caller returns instead of asserting.
async fn live_feed() -> Option<String> {
    let url = feed_url("frieren");
    let client = match reqwest::Client::builder().user_agent(UA).build() {
        Ok(client) => client,
        Err(err) => {
            println!("SKIP: no HTTP client could be built: {}", err);
            return None;
        }
    };
    let response = match client.get(&url).send().await {
        Ok(response) => response,
        Err(err) => {
            println!("SKIP: nyaa is unreachable from this network: {}", err);
            return None;
        }
    };
    let status = response.status();
    if !status.is_success() {
        println!(
            "SKIP: nyaa answered HTTP {} for {} -- ddos-guard is not letting \
             this network through (parser coverage: nyaa_parse_tests.rs)",
            status, url
        );
        return None;
    }
    let body = response.text().await.ok()?;
    if !body.contains("<rss") {
        let head: String = body.chars().take(120).collect();
        println!("SKIP: nyaa answered a non-feed document: {}", head);
        return None;
    }
    Some(body)
}

fn well_formed(row: &TorrentItem) -> Vec<String> {
    let mut problems = Vec::new();
    if row.info_hash.len() != 40 {
        problems.push(format!("hash: {:?}", row.info_hash));
    }
    if row.size_bytes == 0 {
        problems.push("size: 0".to_string());
    }
    if row.added <= 0 {
        problems.push("date: missing".to_string());
    }
    if !row.download_url.starts_with("https://nyaa.si/download/") {
        problems.push(format!("download_url: {}", row.download_url));
    }
    if !row.page_url.starts_with("https://nyaa.si/view/") {
        problems.push(format!("page_url: {}", row.page_url));
    }
    match row.group {
        None => {}
        // nyaa's own `1_*` branch is Anime; nothing else may claim it.
        Some(Group::Anime) => {}
        Some(other) => problems.push(format!("group: {:?}", other)),
    }
    problems
}

#[tokio::test]
#[ignore = "requires network access to nyaa.si"]
async fn live_rows_carry_every_field_the_source_promised() {
    let Some(feed) = live_feed().await else {
        return;
    };
    let rows = parse_items(&feed).expect("the live feed parses");
    println!("rows={}", rows.len());
    assert!(!rows.is_empty(), "the live feed put no usable row on the page");

    let mut bad = 0;
    for row in &rows {
        let problems = well_formed(row);
        if !problems.is_empty() {
            bad += 1;
            println!("  {}: {}", row.title, problems.join(", "));
        }
    }
    // One odd row must not condemn the page, but a page of odd rows
    // would mean the markup moved.
    assert!(bad * 10 <= rows.len(), "{} of {} rows look wrong", bad, rows.len());
    assert!(rows.iter().all(|r| r.magnet.is_some()), "magnet from the hash");
}

#[tokio::test]
#[ignore = "requires network access to nyaa.si"]
async fn live_search_offers_one_page_and_no_cursor() {
    if live_feed().await.is_none() {
        return;
    }
    let nyaa = NyaaSearcher::new();
    let page = nyaa
        .search(&SearchRequest::new("frieren", 0))
        .await
        .expect("live nyaa search");
    println!(
        "rows={}, has_more={}, next_offset={:?}",
        page.items.len(),
        page.has_more,
        page.next_offset
    );

    assert!(!page.items.is_empty(), "live nyaa search returned nothing");
    // The decision under test: paging was never answered live, so the
    // page must not promise a second one.
    assert!(!page.has_more, "no cursor has been verified for this feed");
    assert_eq!(page.next_offset, None);
}

#[tokio::test]
#[ignore = "requires network access to nyaa.si"]
async fn live_the_torrent_link_the_feed_ships_actually_answers() {
    let Some(feed) = live_feed().await else {
        return;
    };
    let rows = parse_items(&feed).expect("the live feed parses");
    let url = &rows.first().expect("at least one row").download_url;
    println!("GET {}", url);

    let client = match reqwest::Client::builder().user_agent(UA).build() {
        Ok(client) => client,
        Err(err) => panic!("no HTTP client could be built: {}", err),
    };
    let response = match client.get(url).send().await {
        Ok(response) => response,
        // Same honesty as the preflight: unreachable here is not
        // evidence that the link is wrong.
        Err(err) => {
            println!("SKIP: nyaa did not answer the link: {}", err);
            return;
        }
    };
    let status = response.status();
    if !status.is_success() {
        println!("SKIP: the link answered HTTP {}", status);
        return;
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = response.bytes().await.expect("link body");
    println!("status={} type={} bytes={}", status, content_type, bytes.len());

    assert!(!bytes.is_empty(), "an empty .torrent would reach TorrServer");
    assert!(
        bytes.first() != Some(&b'<'),
        "a block page must never be handed over as a .torrent"
    );
    assert_eq!(bytes.first(), Some(&b'd'), "a torrent file is a bencoded dict");
}
