use doris::search::rutor::{count_title_links, parse_results, split_query, title_has_word};

// A reconstructed snippet matching the row shape confirmed by fetching
// live rutor search results pages while writing the parser (see the
// module doc comment on rutor.rs) -- not literally saved off the wire,
// but every field (hrefs, size format, seed/leech icon+number pattern,
// date format) matches what was actually observed there. URL assertions
// below expect `rutor.info` because that is the source's BASE.
const SAMPLE_ROW: &str = r#"
<table>
<tr class="gai">
  <td>07 Сен 25</td>
  <td width="30">
    <a href="/download/1052257"><img src="/d.gif" alt="D"></a>
    <a href="/magnet/1052257"><img src="/m.png" alt="M"></a>
    <a href="/torrent/1052257">Остров забвения (2009) WEB-DL 1080p | D</a>
  </td>
  <td align="right">2.27&nbsp;GB</td>
  <td>
    <img src="arrowup.gif" alt="S"> 6
    <img src="arrowdown.gif" alt="L"> 2
  </td>
</tr>
<tr class="tum">
  <td>08 Июн 25</td>
  <td width="30">
    <a href="/download/1040722"><img src="/d.gif" alt="D"></a>
    <a href="/magnet/1040722"><img src="/m.png" alt="M"></a>
    <a href="/torrent/1040722">Зеркало (1974) WEB-DLRip 720p</a>
  </td>
  <td align="right">3.11 GB</td>
  <td>
    <img src="arrowup.gif" alt="S"> 0
    <img src="arrowdown.gif" alt="L"> 0
  </td>
</tr>
</table>
"#;

#[test]
fn test_parses_both_rows() {
    let items = parse_results(SAMPLE_ROW);
    assert_eq!(items.len(), 2);
}

#[test]
fn test_extracts_title_correctly() {
    let items = parse_results(SAMPLE_ROW);
    assert_eq!(items[0].title, "Остров забвения (2009) WEB-DL 1080p | D");
    assert_eq!(items[1].title, "Зеркало (1974) WEB-DLRip 720p");
}

#[test]
fn test_download_and_page_urls_use_the_numeric_id() {
    let items = parse_results(SAMPLE_ROW);
    assert_eq!(items[0].download_url, "https://rutor.info/download/1052257");
    assert_eq!(items[0].page_url, "https://rutor.info/torrent/1052257");
    assert_eq!(items[1].download_url, "https://rutor.info/download/1040722");
}

#[test]
fn test_extracts_size() {
    let items = parse_results(SAMPLE_ROW);
    assert!(items[0].size.contains("2.27"));
    assert!(items[0].size.contains("GB"));
    assert!(items[1].size.contains("3.11"));
}

#[test]
fn test_extracts_seeds() {
    let items = parse_results(SAMPLE_ROW);
    assert_eq!(items[0].seeds, "6");
    assert_eq!(items[1].seeds, "0");
}

#[test]
fn test_extracts_date() {
    let items = parse_results(SAMPLE_ROW);
    assert_eq!(items[0].date, "07 Сен 25");
    assert_eq!(items[1].date, "08 Июн 25");
}

#[test]
fn test_empty_html_returns_no_items() {
    assert!(parse_results("").is_empty());
    assert!(parse_results("<html><body>No results</body></html>").is_empty());
}

#[test]
fn test_row_missing_size_or_seeds_does_not_panic() {
    let minimal = r#"
        <table><tr>
            <td>01 Янв 26</td>
            <td><a href="/torrent/999">Bare title only</a></td>
        </tr></table>
    "#;
    let items = parse_results(minimal);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "Bare title only");
    assert_eq!(items[0].size, "");
    assert_eq!(items[0].seeds, "");
}

#[test]
fn test_duplicate_torrent_link_in_same_row_only_counted_once() {
    let dup = r#"
        <table><tr>
            <td><a href="/torrent/555">Same Torrent</a> <a href="/torrent/555">Same Torrent</a></td>
        </tr></table>
    "#;
    let items = parse_results(dup);
    assert_eq!(items.len(), 1);
}

#[test]
fn test_non_numeric_torrent_id_is_ignored() {
    let bad = r#"<table><tr><td><a href="/torrent/abc">Not a real id</a></td></tr></table>"#;
    assert!(parse_results(bad).is_empty());
}

#[test]
fn test_count_title_links_matches_parse_results_count_on_valid_rows() {
    assert_eq!(count_title_links(SAMPLE_ROW), 2);
    assert_eq!(count_title_links(SAMPLE_ROW), parse_results(SAMPLE_ROW).len());
}

#[test]
fn test_count_title_links_zero_on_challenge_or_error_page() {
    // Simulates what search_page's diagnostic check is looking for: a
    // non-search-results page (e.g. a block/challenge page) has no
    // /torrent/ links at all.
    let challenge_page = "<html><body><h1>Access denied</h1><p>Please verify you are human.</p></body></html>";
    assert_eq!(count_title_links(challenge_page), 0);
}

// Row markup taken from a live rutor.info results page fetched on
// 25.09.2026 (indentation trimmed and the decorative `class` attributes
// dropped to fit the line limit -- neither matters to the parser). What
// differs from the old rutor.org row and is pinned here: the download
// link is protocol-relative and points at `d.rutor.info`, the title link
// carries a slug after the numeric id, the magnet link is an inline
// `magnet:?xt=urn:btih:...` URI rather than an `/magnet/{id}` endpoint,
// and the date parts are separated by literal `&nbsp;` entities. The
// `<table>` wrapper is required, not decoration: html5ever drops a bare
// `<tr>` outside a table, which would make `enclosing_row` find no
// row at all.
const LIVE_ROW: &str = r#"
<table><tbody>
  <tr class="gai">
    <td>06&nbsp;Сен&nbsp;26</td>
    <td colspan = "2">
      <a class="downgif" href="//d.rutor.info/download/1105259"><img src="//cdnbunny.org/i/d.gif" alt="D" /></a>
      <a href="magnet:?xt=urn:btih:06555d165746e815b0ab5b16de37ed24f9142595&dn=rutor.info&tr=udp://opentor.net:6969"><img src="//cdnbunny.org/i/m.png" alt="M" /></a>
      <a href="/torrent/1105259/bad-matrix-dangerous-game-2026-mp3">Bad Matrix - Dangerous Game (2026) MP3 </a>
    </td>
    <td align="right">82.73&nbsp;MB</td>
    <td align="center"><span class="green"><img src="//cdnbunny.org/t/arrowup.gif" alt="S" />&nbsp;1</span>&nbsp;<img src="//cdnbunny.org/t/arrowdown.gif" alt="L" /><span class="red">&nbsp;0</span></td>
  </tr>
</tbody></table>
"#;

#[test]
fn test_parses_live_row_shape() {
    let items = parse_results(LIVE_ROW);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "Bad Matrix - Dangerous Game (2026) MP3");
    assert_eq!(items[0].size, "82.73 MB");
    // `&nbsp;`-separated on rutor.info, normalized back to plain spaces.
    assert_eq!(items[0].date, "06 Сен 26");
}

#[test]
fn test_extracts_seeds_from_live_nbsp_markup() {
    // The regression this whole investigation started from: seeds were
    // empty on every real row because the digits sit behind `&nbsp;`.
    let items = parse_results(LIVE_ROW);
    assert_eq!(items[0].seeds, "1");
}

#[test]
fn test_split_query_drops_stopwords_and_short_words() {
    let (kept, dropped) = split_query("world war z");
    assert_eq!(kept, vec!["world", "war"]);
    assert_eq!(dropped, vec!["z"]);

    let (kept, dropped) = split_query("the Matrix");
    assert_eq!(kept, vec!["Matrix"]);
    assert_eq!(dropped, vec!["the"]);

    let (kept, dropped) = split_query("i am legend");
    assert_eq!(kept, vec!["legend"]);
    assert_eq!(dropped, vec!["i", "am"]);
}

#[test]
fn test_split_query_keeps_normal_queries_untouched() {
    let (kept, dropped) = split_query("Blade Runner 2049");
    assert_eq!(kept, vec!["Blade", "Runner", "2049"]);
    assert!(dropped.is_empty());
}

#[test]
fn test_split_query_trims_attached_punctuation() {
    let (kept, dropped) = split_query("the, matrix!");
    assert_eq!(kept, vec!["matrix"]);
    assert_eq!(dropped, vec!["the"]);
}

#[test]
fn test_split_query_punctuation_only_word_is_dropped_not_kept() {
    // A stray "-" must not survive into the relaxed query.
    let (kept, dropped) = split_query("matrix -");
    assert_eq!(kept, vec!["matrix"]);
    assert_eq!(dropped, vec![""]);
}

#[test]
fn test_split_query_all_words_dropped_yields_empty_kept() {
    // Callers must check `kept`: nothing left to search with.
    let (kept, dropped) = split_query("it");
    assert!(kept.is_empty());
    assert_eq!(dropped, vec!["it"]);
}

#[test]
fn test_title_has_word_matches_whole_words_only() {
    assert!(title_has_word("Матрица / The Matrix (1999)", "the"));
    assert!(title_has_word("Матрица / The Matrix (1999)", "Matrix"));
    assert!(title_has_word("Матрица / The Matrix (1999)", "matrix"));
    assert!(!title_has_word("Theatre (2020)", "the"));
    assert!(!title_has_word("Матрица (1999)", "the"));
    assert!(title_has_word("World War Z (2013)", "z"));
    assert!(!title_has_word("World Zoo (2013)", "z"));
}

#[test]
fn test_title_has_word_empty_word_never_matches() {
    assert!(!title_has_word("Anything", ""));
}

// Rows served to Russian-language clients spell the unit in Cyrillic,
// which the original `(TB|GB|MB|KB)` pattern missed entirely: `size`
// came back empty for `2,27 ГБ` (B0.6).
const CYRILLIC_SIZE_ROW: &str = r#"
<table>
<tr class="gai">
  <td>07 Сен 25</td>
  <td width="30">
    <a href="/torrent/1052257">Игра престолов (2019) WEB-DL 1080p</a>
  </td>
  <td align="right">2,27&nbsp;ГБ</td>
  <td>
    <img src="arrowup.gif" alt="S"> 14
    <img src="arrowdown.gif" alt="L"> 3
  </td>
</tr>
<tr class="tum">
  <td>08 Июн 25</td>
  <td width="30">
    <a href="/torrent/1052258">Мелкий ремонт (2024) HDRip</a>
  </td>
  <td align="right">750 мб</td>
  <td>
    <img src="arrowup.gif" alt="S"> 1
    <img src="arrowdown.gif" alt="L"> 0
  </td>
</tr>
</table>
"#;

#[test]
fn test_extracts_size_with_cyrillic_units() {
    let items = parse_results(CYRILLIC_SIZE_ROW);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].size, "2.27 ГБ", "comma decimal + Cyrillic unit");
    assert_eq!(items[1].size, "750 мб", "lower-case Cyrillic unit");
}

#[test]
fn test_cyrillic_size_does_not_break_seeds_or_date() {
    let items = parse_results(CYRILLIC_SIZE_ROW);
    assert_eq!(items[0].seeds, "14");
    assert_eq!(items[0].date, "07 Сен 25");
}

#[test]
fn test_live_row_urls_are_built_from_the_numeric_id_plus_base() {
    // rutor.info title links carry a slug (`/torrent/{id}/{slug}`) and
    // protocol-relative `//d.rutor.info/download/{id}` hrefs; the parser
    // takes the first path segment as the id and rebuilds both URLs on
    // BASE, so either mirror's markup yields the same pair.
    let items = parse_results(LIVE_ROW);
    assert_eq!(items[0].download_url, "https://rutor.info/download/1105259");
    assert_eq!(items[0].page_url, "https://rutor.info/torrent/1105259");
}

// rutor.info carries a news table (`table#news_table`) whose links use
// the same `/torrent/{id}` shape as real results, with ids like 472 --
// they have no size/seeds/date and must never show up as torrents. This
// leaked through as soon as the source moved to rutor.info, because
// there those hrefs are relative and the title-link selector matches
// them (on rutor.org they were absolute).
const NEWS_AND_RESULT: &str = r#"
<table id="news_table">
  <tr><td colspan="2"><strong>Новости трекера</strong></td></tr>
  <tr><td class="news_date">22-Апр</td>
    <td class="news_title"><a href="/torrent/472" id="news89">Новый Адрес: RUTOR.INFO</a></td></tr>
</table>
<table><tbody>
  <tr class="gai">
    <td>06&nbsp;Сен&nbsp;26</td>
    <td colspan = "2">
      <a href="/torrent/1105259/bad-matrix-dangerous-game-2026-mp3">Bad Matrix - Dangerous Game (2026) MP3 </a>
    </td>
    <td align="right">82.73&nbsp;MB</td>
    <td align="center"><span class="green"><img src="//cdnbunny.org/t/arrowup.gif" alt="S" />&nbsp;1</span></td>
  </tr>
</tbody></table>
"#;

#[test]
fn test_news_table_rows_are_not_results() {
    let items = parse_results(NEWS_AND_RESULT);
    assert_eq!(items.len(), 1, "news links must be skipped");
    assert_eq!(items[0].title, "Bad Matrix - Dangerous Game (2026) MP3");
    assert_eq!(items[0].download_url, "https://rutor.info/download/1105259");
}

#[test]
fn test_news_row_does_not_shadow_a_real_result_with_the_same_id() {
    // The id filter runs after the row-class filter: a news entry whose
    // id happened to match a real result must not consume it.
    let html = NEWS_AND_RESULT.replace("/torrent/472", "/torrent/1105259");
    let items = parse_results(&html);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "Bad Matrix - Dangerous Game (2026) MP3");
}

// --- B1 numeric/hash fields ------------------------------------------------

#[test]
fn test_live_row_fills_numeric_and_hash_fields() {
    let items = parse_results(LIVE_ROW);
    let it = &items[0];
    assert_eq!(it.size_bytes, 82_730_000, "82.73 MB in bytes");
    assert_eq!(it.seeds_n, 1);
    assert_eq!(it.leechers, 0, "the row's peer count was 0");
    assert_eq!(it.added, 1_788_652_800, "06 Сен 26 as unix seconds (UTC)");
    assert_eq!(it.info_hash, "06555d165746e815b0ab5b16de37ed24f9142595");
    let magnet = it.magnet.as_deref().expect("row has an inline magnet");
    assert!(magnet.starts_with("magnet:?xt=urn:btih:06555d165746e815"));
    assert!(
        magnet.contains("&dn=rutor.info"),
        "query parts must not come back HTML-escaped: {}",
        magnet
    );
    assert_eq!(it.group, None, "category 0 = 'all', nothing to attribute");
}

#[test]
fn test_sample_rows_fill_leechers_and_added() {
    let items = parse_results(SAMPLE_ROW);
    // Row 1: `<img alt="L"> 2` peers; row 2: none.
    assert_eq!(items[0].leechers, 2);
    assert_eq!(items[1].leechers, 0);
    assert_eq!(items[0].seeds_n, 6);
    // "07 Сен 25" / "08 Июн 25" -> UTC midnight of that day.
    assert_eq!(items[0].added, 1_757_203_200);
    assert_eq!(items[1].added, 1_749_340_800);
    assert_eq!(items[0].size_bytes, 2_270_000_000);
    assert_eq!(items[1].size_bytes, 3_110_000_000);
}

#[test]
fn test_row_without_magnet_leaves_hash_and_magnet_empty() {
    // rutor.org's rows only had an `/magnet/{id}` endpoint, not an inline
    // magnet URI -- such rows must not invent a hash.
    let items = parse_results(SAMPLE_ROW);
    assert_eq!(items[0].magnet, None);
    assert_eq!(items[0].info_hash, "");
}

#[test]
fn test_base32_info_hash_is_left_for_the_magnet_pipeline() {
    // A 32-char base32 btih is a real hash, but converting it to hex is
    // B7's `normalize_info_hash` job -- filling it in half-way here would
    // make dedup (B4) compare a base32 hash against a hex one.
    let html = r#"
    <table><tr class="gai">
      <td>06 Сен 26</td>
      <td colspan="2">
        <a href="magnet:?xt=urn:btih:ABCDEF234567890ABCDEF234567890AB"><img alt="M"></a>
        <a href="/torrent/42">Base32 Row</a>
      </td>
      <td align="right">1 GB</td>
    </tr></table>
    "#;
    let items = parse_results(html);
    assert_eq!(items.len(), 1);
    assert!(items[0].magnet.is_some(), "the magnet URI itself is kept");
    assert_eq!(items[0].info_hash, "");
}
