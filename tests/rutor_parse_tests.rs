use doris::search::rutor::{count_title_links, parse_results, split_query, title_has_word};

// A reconstructed snippet matching the row shape confirmed by fetching a
// live rutor.org search results page while writing the parser (see the
// module doc comment on rutor.rs) -- not literally saved off the wire,
// but every field (hrefs, size format, seed/leech icon+number pattern,
// date format) matches what was actually observed there.
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
    assert_eq!(items[0].download_url, "https://rutor.org/download/1052257");
    assert_eq!(items[0].page_url, "https://rutor.org/torrent/1052257");
    assert_eq!(items[1].download_url, "https://rutor.org/download/1040722");
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

// Row markup taken from a live rutor.org results page fetched while
// fixing the zero-results bug (indentation trimmed and the decorative
// `class` attributes dropped to fit the line limit -- neither matters to
// the parser): the counts are wrapped as `alt="S">&nbsp;0` and
// `alt="L"><span class="red">&nbsp;0</span>`, which is what the old
// `\s*`-based seed regex failed to match (seeds used to come back empty
// on every real result). The `<table>` wrapper is required, not
// decoration: html5ever drops a bare `<tr>` outside a table, which would
// make `enclosing_row_html` find no row at all.
const LIVE_ROW: &str = r#"
<table><tbody>
  <tr class="gai">
    <td>22 Сен 25</td>
    <td colspan="2">
      <a href="https://rutor.org/download/1054060"><img src="/d.gif" alt="D"></a>
      <a href="https://rutor.org/magnet/1054060"><img src="/m.png" alt="M"></a>
      <a href="/torrent/1054060">Sleepwell Citizen - This Is Only A Test (2025)</a>
    </td>
    <td align="right">528.42 MB</td>
    <td align="center" class="nowrap">
      <span class="green"><img src="/arrowup.gif" alt="S">&nbsp;42</span>
      &nbsp;<img src="/arrowdown.gif" alt="L"><span class="red">&nbsp;7</span>
    </td>
  </tr>
</tbody></table>
"#;

#[test]
fn test_parses_live_row_shape() {
    let items = parse_results(LIVE_ROW);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "Sleepwell Citizen - This Is Only A Test (2025)");
    assert_eq!(items[0].size, "528.42 MB");
    assert_eq!(items[0].date, "22 Сен 25");
    assert_eq!(items[0].download_url, "https://rutor.org/download/1054060");
}

#[test]
fn test_extracts_seeds_from_live_nbsp_markup() {
    // The regression this whole investigation started from: seeds were
    // empty on every real row because the digits sit behind `&nbsp;`.
    let items = parse_results(LIVE_ROW);
    assert_eq!(items[0].seeds, "42");
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
