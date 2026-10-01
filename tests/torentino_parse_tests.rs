//! Offline tests for the torentino source: the row parser, the date
//! normalization, the pagination verdict and the item-page download
//! link, all against fixtures shaped like the live markup (probed
//! 26.09.2026 -- see the module doc).

use doris::sources::source::Group;
use doris::sources::torentino::{
    find_download_link, has_next_page, parse_date, parse_results, TorentinoSearcher,
};

/// One results row, in the shape the live search page answers with.
const ROW: &str = concat!(
    r#"<div id="entryID3689"><div class="shortstory">"#,
    r#"<div class="short_poster"><a href="/load/adventure/the_matrix/9-1-0-3689">"#,
    r#"<img src="/_ld/36/3689.jpg" alt=""></a></div>"#,
    r#"<div class="short_descr"><h2>"#,
    r#"<a href="/load/adventure/the_matrix/9-1-0-3689">"#,
    r#"The Matrix Awakens: An Unreal Engine 5 Experience</a></h2>"#,
    r#"<div class="short_cat"> <a href="/load/adventure/9">Adventure / Приключения</a>"#,
    r#" <span> | Дата: 29.08.2026, 11:26</span>"#,
    r#" <span> | Просмотров: 125</span> <span> | Комментариев: 0</span></div>"#,
    r#"</div>"#,
    r#"<div class="short_info"><div class="size_file">1.2 GB</div>"#,
    r#"<div class="btn_more"><a href="/load/adventure/the_matrix/9-1-0-3689" "#,
    r#"class="btn">Скачать торрент</a></div></div>"#,
    r#"</div></div>"#,
);

/// The pagination block of a single-page answer: the current page is a
/// `<b>`, and there is no link to a next one.
const PAGES_SINGLE: &str = concat!(
    r#"<div class="navigation ignore-select">"#,
    r#"<div class="pages"><span class="pagesBlockuz1"><b class="swchItemA">"#,
    r#"<span>1</span></b></span></div>"#,
    r#"</div>"#,
);

/// A pagination block that does offer a next page.
const PAGES_MORE: &str = concat!(
    r#"<div class="navigation ignore-select">"#,
    r#"<div class="pages"><span class="pagesBlockuz1"><b class="swchItemA">"#,
    r#"<span>1</span></b></span>"#,
    r#"<span class="pagesBlockuz"><a href="/load/page/2/">2</a></span></div>"#,
    r#"</div>"#,
);

/// An item page carrying the file link `download_torrent` follows.
const ITEM_PAGE: &str = concat!(
    r#"<div class="size_file"><i class="ion-ios-cloud-download-outline"></i> 19.6 Kb</div>"#,
    r#"<div class="btn_more"><a href="/load/0-0-0-110-20" >Скачать</a></div>"#,
    r#"<div class="count_download">Загрузок: 208</div>"#,
);

#[test]
fn test_a_row_is_read_for_title_link_date_size_and_group() {
    let items = parse_results(ROW);
    assert_eq!(items.len(), 1, "one entryID block must give one row");
    let item = &items[0];
    assert_eq!(
        item.title,
        "The Matrix Awakens: An Unreal Engine 5 Experience"
    );
    assert_eq!(
        item.page_url, "/load/adventure/the_matrix/9-1-0-3689",
        "the row links its item page"
    );
    assert_eq!(
        item.download_url, item.page_url,
        "the file link lives on the item page, so the row points there"
    );
    assert_eq!(
        item.date, "2026-08-29",
        "the date is normalized, not transliterated"
    );
    assert_eq!(item.size, "1.2 GB");
    assert_eq!(
        item.group,
        Some(Group::Games),
        "a games tracker claims Games"
    );
    // No magnet and no hash on this site: playback is the.torrent path.
    assert!(item.magnet.is_none());
    assert!(item.info_hash.is_empty());
}

#[test]
fn test_a_row_without_a_title_or_link_is_skipped() {
    let html = concat!(
        r#"<div id="entryID1"><div class="shortstory">"#,
        r#"<div class="short_descr"><h2></h2></div></div></div>"#,
    );
    assert!(
        parse_results(html).is_empty(),
        "a row with no title link is not a row"
    );
}

#[test]
fn test_the_date_normalizes_to_iso() {
    assert_eq!(
        parse_date(" | Дата: 29.08.2026, 11:26").as_deref(),
        Some("2026-08-29")
    );
    assert_eq!(
        parse_date(" | Дата: 01.01.2027").as_deref(),
        Some("2027-01-01")
    );
    assert_eq!(parse_date("no date here"), None);
}

#[test]
fn test_the_pagination_verdict_follows_the_block() {
    assert!(
        !has_next_page(PAGES_SINGLE),
        "a block with no links is a single-page answer -- what every live probe showed"
    );
    assert!(
        has_next_page(PAGES_MORE),
        "a block with a page link has a next page"
    );
}

#[test]
fn test_the_download_link_is_the_item_pages_file_link() {
    let link = find_download_link(ITEM_PAGE).expect("the item page carries the link");
    assert_eq!(
        link, "https://torentino.org/load/0-0-0-110-20",
        "the link is resolved against the site root"
    );
    assert!(
        find_download_link("<div>no link</div>").is_none(),
        "an item page without the link is reported, not guessed"
    );
}

#[test]
fn test_the_searcher_is_constructible_offline() {
    // The same shape the other sources pin: `new()` must not need a
    // browser or a session, so the registry can build it -- and the
    // searcher must agree with the row `KNOWN_SOURCES` filed it under.
    // An `id()` that disagrees leaves it unreachable behind its own
    // row, and constructing it would never show that.
    use doris::sources::source::{self, Source};
    let searcher = TorentinoSearcher::new();
    let info = source::get_source("torentino").expect("torentino must be registered");
    assert!(info.implemented, "the Options checklist prints this flag");
    assert_eq!(searcher.id(), info.id);
    assert_eq!(searcher.label(), info.label);
    assert_eq!(searcher.groups(), info.groups);
    assert_eq!(searcher.requires_browser(), info.requires_browser);
    assert!(searcher.home_url().starts_with("http"), "{}", info.home_url);
}
