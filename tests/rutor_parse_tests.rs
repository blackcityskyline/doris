use doris::search::rutor::parse_results;

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
