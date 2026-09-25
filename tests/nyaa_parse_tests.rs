//! Offline tests for the nyaa RSS parser, built from the one feed wave
//! 2 actually captured live on 25.09.2026 (75 items) -- including the
//! two things that must never be guessed: what a row carries, and what
//! the source refuses to promise while the host stays unreachable.
//!
//! The fixture below keeps nyaa's real spelling: entities in `<title>`
//! (never CDATA), `nyaa:size` as a human string, `<guid isPermaLink>`
//! with an attribute, and a `-0000` offset on `<pubDate>`.

use doris::search::nyaa::{
    NyaaSearcher, feed_url, parse_items, to_page, unescape_entities,
};
use doris::search::models::TorrentItem;
use doris::search::source::{Group, SearchRequest, Source};

/// Six items, four of which may become rows: two lack a usable hash
/// and are dropped one row each (the parser's own rule).
const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:nyaa="https://nyaa.si/xmlns/nyaa">
	<channel>
		<title>Nyaa - &#34;frieren&#34; - Torrent File RSS</title>
		<link>https://nyaa.si/</link>
		<atom:link href="https://nyaa.si/?page=rss" rel="self" type="application/rss+xml" />
		<item>
			<title>[Erai-raws] Sousou no Frieren 2nd Season [1080p CR WEBRip]</title>
			<link>https://nyaa.si/download/2160092.torrent</link>
			<guid isPermaLink="true">https://nyaa.si/view/2160092</guid>
			<pubDate>Sat, 12 Sep 2026 15:45:00 -0000</pubDate>
			<nyaa:seeders>36</nyaa:seeders>
			<nyaa:leechers>3</nyaa:leechers>
			<nyaa:downloads>629</nyaa:downloads>
			<nyaa:infoHash>ef3e7ad1b12bdd9fc341691d8866cd1fa8374a4b</nyaa:infoHash>
			<nyaa:categoryId>1_2</nyaa:categoryId>
			<nyaa:category>Anime - English-translated</nyaa:category>
			<nyaa:size>6.6 GiB</nyaa:size>
			<nyaa:trusted>No</nyaa:trusted>
			<description><![CDATA[<a href="https://nyaa.si/view/2160092">#2160092</a>]]></description>
		</item>
		<item>
			<title>Frieren: Beyond Journey&#39;s End &amp; OVA &gt; Special [BD]</title>
			<link>https://nyaa.si/download/2154329.torrent</link>
			<guid isPermaLink="true">https://nyaa.si/view/2154329</guid>
			<pubDate>Mon, 31 Aug 2026 12:32:25 -0000</pubDate>
			<nyaa:seeders>19</nyaa:seeders>
			<nyaa:leechers>0</nyaa:leechers>
			<nyaa:infoHash>a47b68971cdb82a4ef05ca239317cdef99bd2ca0</nyaa:infoHash>
			<nyaa:categoryId>1_3</nyaa:categoryId>
			<nyaa:category>Anime - Non-English-translated</nyaa:category>
			<nyaa:size>1.45 GiB</nyaa:size>
		</item>
		<item>
			<title>Frieren Original Soundtrack &amp; Drama CD</title>
			<link>https://nyaa.si/download/2150001.torrent</link>
			<guid isPermaLink="true">https://nyaa.si/view/2150001</guid>
			<pubDate>Sun, 13 Sep 2026 00:12:00 +0000</pubDate>
			<nyaa:seeders>0</nyaa:seeders>
			<nyaa:leechers>1</nyaa:leechers>
			<nyaa:infoHash>0f1e2d3c4b5a69788796a5b4c3d2e1f001122334</nyaa:infoHash>
			<nyaa:categoryId>2_1</nyaa:categoryId>
			<nyaa:category>Audio - Lossless</nyaa:category>
			<nyaa:size>350 MiB</nyaa:size>
		</item>
		<item>
			<title>Row without any hash at all</title>
			<link>https://nyaa.si/download/2149999.torrent</link>
			<guid isPermaLink="true">https://nyaa.si/view/2149999</guid>
			<pubDate>Sun, 13 Sep 2026 00:13:00 +0000</pubDate>
			<nyaa:seeders>4</nyaa:seeders>
			<nyaa:categoryId>1_1</nyaa:categoryId>
			<nyaa:size>700 MiB</nyaa:size>
		</item>
		<item>
			<title>Row whose hash is not a hash</title>
			<link>https://nyaa.si/download/2149998.torrent</link>
			<guid isPermaLink="true">https://nyaa.si/view/2149998</guid>
			<pubDate>Sun, 13 Sep 2026 00:14:00 +0000</pubDate>
			<nyaa:seeders>4</nyaa:seeders>
			<nyaa:infoHash>not-a-hash</nyaa:infoHash>
			<nyaa:categoryId>1_1</nyaa:categoryId>
			<nyaa:size>700 MiB</nyaa:size>
		</item>
		<item>
			<title>Row whose date nyaa could not spell</title>
			<link>https://nyaa.si/download/2149997.torrent</link>
			<guid isPermaLink="true">https://nyaa.si/view/2149997</guid>
			<pubDate>Sometime soon</pubDate>
			<nyaa:seeders>7</nyaa:seeders>
			<nyaa:infoHash>ef3e7ad1b12bdd9fc341691d8866cd1fa8374a4c</nyaa:infoHash>
			<nyaa:categoryId>1_4</nyaa:categoryId>
			<nyaa:size>2.5 GiB</nyaa:size>
		</item>
	</channel>
</rss>
"#;

fn rows() -> Vec<TorrentItem> {
    parse_items(FEED).expect("the captured feed parses")
}

#[test]
fn test_every_field_the_live_feed_shipped_is_on_the_row() {
    let rows = rows();
    assert_eq!(rows.len(), 4, "four of the six items may become rows");

    let first = &rows[0];
    assert_eq!(
        first.title,
        "[Erai-raws] Sousou no Frieren 2nd Season [1080p CR WEBRip]"
    );
    assert_eq!(first.info_hash, "ef3e7ad1b12bdd9fc341691d8866cd1fa8374a4b");
    assert_eq!(first.seeds, "36");
    assert_eq!(first.seeds_n, 36);
    assert_eq!(first.leechers, 3);
    // "6.6 GiB" is binary, and the display column re-renders bytes the
    // way every other source does, so yts and nyaa read the same shape.
    assert_eq!(first.size_bytes, (6.6 * 1024.0_f64.powi(3)).round() as u64);
    assert_eq!(first.size, "6.60 GB");
    assert_eq!(first.date, "2026-09-12");
    assert!(first.added > 0, "the -0000 offset must still give an instant");
    assert_eq!(first.download_url, "https://nyaa.si/download/2160092.torrent");
    assert_eq!(first.page_url, "https://nyaa.si/view/2160092");
    assert_eq!(first.source, "nyaa");

    // nyaa ships hashes rather than magnets (0 of 75 live), so the row
    // builds one from the verified hash plus the shared trackers.
    let magnet = first.magnet.as_deref().expect("a magnet is built");
    assert!(
        magnet.starts_with("magnet:?xt=urn:btih:ef3e7ad1b12bdd9fc341691d8866cd1fa8374a4b"),
        "the hash travels into the magnet: {}",
        magnet
    );
    assert!(magnet.contains("&dn="), "and the title: {}", magnet);
    assert!(magnet.contains("&tr="), "and trackers: {}", magnet);
}

#[test]
fn test_entities_come_out_as_text_not_as_markup() {
    let rows = rows();
    assert_eq!(
        rows[1].title,
        "Frieren: Beyond Journey's End & OVA > Special [BD]"
    );

    // The whole table torio's `unescapeEntities` spells out, so a
    // future feed surprise lands in one known place.
    assert_eq!(
        unescape_entities("a &amp; b &#39;c&#039; d &apos;e&apos; f"),
        "a & b 'c' d 'e' f"
    );
    assert_eq!(
        unescape_entities("&#34;quoted&#34; &quot;too&quot; &lt;tag&gt;"),
        "\"quoted\" \"too\" <tag>"
    );
    assert_eq!(unescape_entities("dash &#8211; and &#8212;"), "dash - and -");
    assert_eq!(unescape_entities("curly &#8217;s &#8220;q&#8221;"), "curly 's \"q\"");
}

#[test]
fn test_a_group_comes_from_the_items_own_category_not_from_the_query() {
    let rows = rows();
    // All four rows were requested under one `c=0_0` query, so the
    // group has to come from nyaa's own `nyaa:categoryId`.
    assert_eq!(rows[0].group, Some(Group::Anime), "1_2 is the Anime branch");
    assert_eq!(rows[1].group, Some(Group::Anime));
    assert_eq!(rows[2].group, None, "2_1 is Audio: nothing to claim");
    assert_eq!(rows[3].group, Some(Group::Anime), "1_4 is Anime/Other");

    // What the source *is* still differs from what a row may claim:
    let nyaa = NyaaSearcher::new();
    assert_eq!(nyaa.groups(), &[Group::Anime]);
    assert!(!nyaa.requires_browser());
}

#[test]
fn test_an_unusable_hash_costs_one_row_and_not_the_page() {
    let rows = rows();
    let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
    assert!(
        !titles.contains(&"Row without any hash at all"),
        "no hash means no magnet, and a row with neither is unusable"
    );
    assert!(
        !titles.contains(&"Row whose hash is not a hash"),
        "'not-a-hash' must not reach the table as a 40-char promise"
    );
    assert_eq!(titles.len(), 4, "the other rows still parse");
}

#[test]
fn test_a_date_nyaa_could_not_spell_keeps_the_row_without_inventing_a_day() {
    let rows = rows();
    let undated = rows
        .iter()
        .find(|r| r.title.starts_with("Row whose date"))
        .expect("the row with a broken pubDate still parses");
    assert_eq!(undated.added, 0);
    assert_eq!(undated.date, "", "no 1970-01-01 in the Date column");
    assert_eq!(undated.seeds_n, 7, "everything else on the row is intact");
}

#[test]
fn test_an_error_page_is_rejected_instead_of_reading_as_no_results() {
    // Exactly what ddos-guard served this network while wave 2 was
    // being written: a tiny HTML stub, not a feed.
    let blocked = "<!DOCTYPE html><html lang=en><title>Error 504</title>\
                   <p>upstream timeout</p>";
    let err = parse_items(blocked).expect_err("a block page is not a feed");
    assert!(
        err.to_string().contains("not an RSS feed"),
        "the refusal names the actual problem: {}",
        err
    );
}

#[test]
fn test_the_page_claims_only_one_page_and_no_cursor() {
    // Wave-2 decision: pagination (`&p=2`) was never answered live, so
    // neither claim may be made -- and both are testable offline
    // because the page is built outside `search`.
    let page = to_page(rows());
    assert!(!page.has_more, "no 'load more' into a URL nobody has seen");
    assert_eq!(page.next_offset, None);
    assert_eq!(page.items.len(), 4);
    assert_eq!(doris::search::nyaa::page_items(), 75, "the live page size");
}

#[tokio::test]
async fn test_an_empty_query_is_refused_with_a_reason_before_any_request() {
    // `supports_browse` is false precisely because the empty-query feed
    // was never verified; reaching the network here would mean the
    // refusal moved somewhere it can no longer stop a request.
    let nyaa = NyaaSearcher::new();
    assert!(!nyaa.supports_browse(), "browse stays unclaimed");

    let err = nyaa
        .search(&SearchRequest::new("   ", 0))
        .await
        .expect_err("an unverified browse feed must not be requested");
    let text = err.to_string();
    assert!(text.contains("type a query"), "the reason: {}", text);
}

#[test]
fn test_the_feed_url_asks_for_every_category_and_encodes_the_query() {
    assert_eq!(
        feed_url("frieren"),
        "https://nyaa.si/?page=rss&q=frieren&c=0_0&f=0"
    );
    let spaced = feed_url("frieren s2");
    assert!(
        spaced.contains("q=frieren%20s2"),
        "the query is encoded, not spliced into the URL: {}",
        spaced
    );
    assert!(
        feed_url("").contains("c=0_0"),
        "the all-category scope is part of the URL, not of the query"
    );
}
