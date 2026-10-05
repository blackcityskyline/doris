//! ext.to's page shape, checked without a browser.
//!
//! Every fixture here is a row off the real `?q=dune` answer of 05.10.2026,
//! copied field for field out of the page -- including the awkward parts: an
//! age cell whose text says "2 years ago" and whose `title` carries the date,
//! and a category that is the *second* path link of the "Posted by" line.

use doris::sources::ext::{
    magnet_from_answer, parse_rows, search_url, topic_id, CATEGORY_IDS, EXT_GROUPS, PAGE_SIZE,
};
use doris::sources::source::Group;

/// The rows as `ROWS_SCRIPT` hands them over: three rows of one live answer,
/// one per category the source can attribute.
const ROWS: &str = r#"[
  {
    "title": "Dune: Part Two (2024) 1080p WEBRip x264 2.0 YTS YIFY",
    "href": "/dune-part-two-2024-1080p-webrip-x264-2-0-yts-yify-14902288/",
    "size": "2.76 GB",
    "seeds": "1023",
    "leechers": "183",
    "age_title": "06 April 2024",
    "category": "Movies"
  },
  {
    "title": "Dune.Prophecy.S01E03.1080p.WEB.H264-SuccessfulCrab[TGx]",
    "href": "/dune-prophecy-s01e03-1080p-web-h264-successfulcrab-tgx-15307359/",
    "size": "3 GB",
    "seeds": "966",
    "leechers": "114",
    "age_title": "02 December 2024",
    "category": "TV"
  },
  {
    "title": "Dune.Awakening.Ultimate.Edition.v1.5.3.0.16-DLCS-Bonuses-FitGirl",
    "href": "/dune-awakening-ultimate-edition-v1-5-3-0-16-dlcs-bonuses-fitgirl-15343355/",
    "size": "591 MB",
    "seeds": "907",
    "leechers": "287",
    "age_title": "23 December 2024",
    "category": "Games"
  }
]"#;

#[test]
fn test_the_url_is_the_browse_slots_the_site_itself_links_to() {
    assert_eq!(
        search_url("dune", 0, None),
        "https://ext.to/browse/?q=dune",
        "a plain query is the site's own /browse/ answer"
    );
    assert_eq!(
        search_url("dune", 0, Some(Group::Movies)),
        "https://ext.to/browse/?q=dune&cat=1",
        "cat=1 is Movies, read off the browse sidebar live"
    );
    assert_eq!(
        search_url("dune", 0, Some(Group::Anime)),
        "https://ext.to/browse/?q=dune&cat=7",
        "Anime is cat=7"
    );
    assert_eq!(
        search_url("dune", PAGE_SIZE, None),
        "https://ext.to/browse/?q=dune&page=2",
        "50 rows a page, so the second page starts at offset 50"
    );
    assert_eq!(
        search_url("dune", PAGE_SIZE * 3 + 7, Some(Group::TV)),
        "https://ext.to/browse/?q=dune&cat=2&page=4",
        "an offset inside the fourth page still asks for page 4"
    );
    assert_eq!(
        search_url("", 0, None),
        "https://ext.to/browse/?q=",
        "an empty query is browse mode, not a refused search"
    );
    assert_eq!(
        search_url("  dune  ", 0, None),
        "https://ext.to/browse/?q=dune",
        "and the query is trimmed before it is encoded"
    );
}

#[test]
fn test_a_row_carries_the_facts_off_the_page_and_neither_link() {
    let rows = parse_rows(ROWS).expect("the fixture is one page's rows");
    assert_eq!(rows.len(), 3);
    let movie = &rows[0];

    assert_eq!(
        movie.title,
        "Dune: Part Two (2024) 1080p WEBRip x264 2.0 YTS YIFY"
    );
    assert_eq!(movie.size, "2.76 GB");
    assert_eq!(
        movie.size_bytes, 2_760_000_000,
        "ext writes Latin units, which doris reads as decimal -- 2.76 GB, not 2.76 GiB"
    );
    assert_eq!(movie.seeds, "1023");
    assert_eq!(movie.seeds_n, 1023);
    assert_eq!(
        movie.leechers, 183,
        "ext shows a Leechs column, so it is read"
    );
    assert_eq!(
        movie.date, "2024-04-06",
        "the age cell's title, not '2 years ago'"
    );
    assert_eq!(movie.source, "ext");
    assert_eq!(
        movie.page_url,
        "https://ext.to/dune-part-two-2024-1080p-webrip-x264-2-0-yts-yify-14902288/",
        "the topic URL is absolute, and it is what the magnet is read from later"
    );
    assert!(
        movie.magnet.is_none(),
        "an anonymous page carries no magnet and a signed-in one hides it behind an \\
         HMAC, so the row must not claim one"
    );
    assert!(
        movie.download_url.is_empty(),
        "and no .torrent URL either -- both players of a row ask the source instead"
    );
}

#[test]
fn test_the_group_comes_off_the_row_not_off_the_request() {
    let rows = parse_rows(ROWS).expect("the fixture parses");
    assert_eq!(rows[0].group, Some(Group::Movies));
    assert_eq!(rows[1].group, Some(Group::TV));
    assert_eq!(rows[2].group, Some(Group::Games));
}

#[test]
fn test_a_category_doris_has_no_group_for_stays_unattributed() {
    // ext has eight categories; Music, Apps, Books and Other have no `Group`.
    let row = r#"[{"title":"Some.Album.FLAC","href":"/some-album-flac-7777777/",
                    "size":"400 MB","seeds":"1","leechers":"0",
                    "age_title":"01 January 2025","category":"Music"}]"#;
    let rows = parse_rows(row).expect("a row of an unfilterable category still parses");
    assert_eq!(
        rows[0].group, None,
        "folding Music into a neighbouring group would be a lie about the row"
    );
    assert_eq!(
        rows[0].size_bytes, 400_000_000,
        "and it is still a row worth showing"
    );
}

#[test]
fn test_a_row_the_page_half_printed_still_arrives() {
    // A label the site renames leaves an empty cell rather than a crash: the
    // numbers are read off labelled cells, so an unknown label is missing data.
    let row = r#"[{"title":"Dune.2021.1080p","href":"/dune-2021-1080p-15343355/",
                    "seeds":"12","category":"Movies"}]"#;
    let rows = parse_rows(row).expect("a row without a size or a date still parses");
    assert_eq!(rows[0].size, "");
    assert_eq!(rows[0].size_bytes, 0);
    assert_eq!(rows[0].date, "");
    assert_eq!(rows[0].seeds_n, 12, "what the page did give is still read");
}

#[test]
fn test_the_topic_id_is_the_number_the_magnet_ajax_wants() {
    assert_eq!(
        topic_id("https://ext.to/dune-part-two-2024-1080p-webrip-x264-2-0-yts-yify-14902288/"),
        Some(14902288),
        "ext spells a topic /<slug>-<id>/ and the id is what the ajax takes"
    );
    assert_eq!(
        topic_id("https://ext.to/dune-prophecy-s01e03-1080p-web-h264-successfulcrab-tgx-15307359"),
        Some(15307359),
        "with or without the trailing slash"
    );
    assert_eq!(topic_id("https://ext.to/browse/?q=dune"), None);
    assert_eq!(
        topic_id("https://ext.to/rules/"),
        None,
        "a slug without a number is not a topic"
    );
    assert_eq!(
        topic_id("https://ext.to/movies-108/"),
        None,
        "a three-digit tail is a word in a slug -- ext's ids are at least four"
    );
}

#[test]
fn test_the_magnet_comes_out_of_the_answers_json_string() {
    // The site's own answer of 05.10.2026, wrapped the way MAGNET_SCRIPT wraps
    // it: the POST answered JSON, the script kept the text, the script's own
    // answer is JSON. So the magnet is two levels down.
    let answer = r#"{"token":true,"csrf":true,"answers":[
      {"endpoint":"/ajax/getTorrentMagnet.php","status":200,
       "text":"{\"success\":true,\"downloads\":18221,\"url\":\"magnet:?xt=urn:btih:2770FE270845674966E184BE60ED1BE0FE494F3A&dn=Dune%3A%20Part%20Two\"}"},
      {"endpoint":"/ajax/getSearchMagnet.php","status":200,
       "text":"{\"success\":false,\"error\":\"Invalid request. Please refresh the page.\"}"}
    ]}"#;
    let magnet = magnet_from_answer(answer)
        .expect("a readable answer")
        .expect("the first endpoint answered");
    assert_eq!(
        magnet,
        "magnet:?xt=urn:btih:2770FE270845674966E184BE60ED1BE0FE494F3A&dn=Dune%3A%20Part%20Two",
        "and the hash inside it is the row's own"
    );
}

#[test]
fn test_a_full_length_magnet_is_read_whole() {
    // A real answer runs to hundreds of characters: a magnet carries every
    // tracker the release was announced on, and the site's own magnet of
    // 05.10.2026 for `Dune: Part Two` is longer than a short sample suggests.
    // Anything that trims the answer before it is decoded -- which is exactly
    // what a JS `slice(0, n)` on the response does -- cuts a JSON document in
    // the middle of a string, and the row then comes back with no magnet and no
    // error to say why.
    let trackers: String = (0..12)
        .map(|i| format!("&tr=udp%3A%2F%2Ftracker%7B7D.example%2Fannounce%2F{i}"))
        .collect();
    let magnet = format!(
        "magnet:?xt=urn:btih:2770FE270845674966E184BE60ED1BE0FE494F3A\
&dn=Dune%3A%20Part%20Two%20(2024)%201080p%20WEBRip%20x264%202.0%20YTS%20YIFY%20%5Bext.to%5D{trackers}"
    );
    assert!(
        magnet.len() > 400,
        "the sample has to be longer than a 400-character cut, or the test is about \
         nothing: {}",
        magnet.len()
    );
    let payload = serde_json::to_string(&serde_json::json!({
        "success": true,
        "url": magnet,
    }))
    .expect("the site's own answer is JSON");
    // The site answers the POST with JSON, and the script kept the *text* of
    // that answer, so `text` holds the payload as a JSON string.
    let payload = serde_json::to_string(&serde_json::Value::String(payload))
        .expect("a string is always encodable");
    let answer = format!(
        r#"{{"token":true,"csrf":true,"answers":[
             {{"endpoint":"/ajax/getTorrentMagnet.php","status":200,"text":{payload}}}
           ]}}"#
    );
    assert_eq!(
        magnet_from_answer(&answer)
            .expect("readable")
            .expect("answered"),
        magnet,
        "a magnet with twelve trackers in it comes back whole"
    );
}

#[test]
fn test_a_refused_magnet_is_none_and_says_why_in_the_log() {
    // What an anonymous page answers: 200, and no session.
    let answer = r#"{"token":false,"csrf":false,"answers":[
      {"endpoint":"/ajax/getTorrentMagnet.php","status":200,
       "text":"{\"success\":false,\"error\":\"Invalid session\"}"}
    ]}"#;
    let err = magnet_from_answer(answer)
        .expect_err("a page with no tokens cannot answer a signed request")
        .to_string();
    assert!(
        err.contains("sign in"),
        "the error has to say what to do: {err}"
    );
}

#[test]
fn test_the_wrong_endpoint_is_not_mistaken_for_the_right_one() {
    // On the search page `getTorrentMagnet.php` answers 200 and refuses in text,
    // and `getSearchMagnet.php` is the one that works. Order decides.
    let refused_first = r#"{"token":true,"csrf":true,"answers":[
      {"endpoint":"/ajax/getTorrentMagnet.php","status":200,
       "text":"{\"success\":false,\"error\":\"Invalid request. Please refresh the page.\"}"},
      {"endpoint":"/ajax/getSearchMagnet.php","status":200,
       "text":"{\"success\":true,\"url\":\"magnet:?xt=urn:btih:C0DAF3C99E437014545434EE7B1E8EE13F4099A8\"}"}
    ]}"#;
    assert_eq!(
        magnet_from_answer(refused_first)
            .expect("readable")
            .expect("the second endpoint answered")
            .split("btih:")
            .nth(1),
        Some("C0DAF3C99E437014545434EE7B1E8EE13F4099A8"),
        "a refusal is skipped, not returned as the answer"
    );

    let all_refused = r#"{"token":true,"csrf":true,"answers":[
      {"endpoint":"/ajax/getTorrentMagnet.php","status":200,
       "text":"{\"success\":false,\"error\":\"Invalid request. Please refresh the page.\"}"}
    ]}"#;
    assert_eq!(
        magnet_from_answer(all_refused).expect("readable"),
        None,
        "nothing to play with is None, which the caller turns into a message"
    );
}

#[test]
fn test_a_non_magnet_url_is_not_handed_over_as_one() {
    let answer = r#"{"token":true,"csrf":true,"answers":[
      {"endpoint":"/ajax/getTorrentMagnet.php","status":200,
       "text":"{\"success\":true,\"url\":\"https://ext.to/download/1/file.torrent\"}"}
    ]}"#;
    assert_eq!(
        magnet_from_answer(answer).expect("readable"),
        None,
        "a .torrent URL is not a magnet, and this source hands the magnet to the \\
         player -- passing it on would look like a magnet link that is not one"
    );
}

#[test]
fn test_the_registry_and_the_category_table_say_the_same_thing() {
    let from_table: Vec<Group> = CATEGORY_IDS.iter().map(|(group, _)| *group).collect();
    assert_eq!(
        from_table, EXT_GROUPS,
        "the groups doris advertises for ext are exactly the categories it can filter \\
         by; a group with no category behind it offers a filter that changes nothing"
    );
    assert!(
        !CATEGORY_IDS.iter().any(|(group, _)| CATEGORY_IDS
            .iter()
            .filter(|(other, _)| other == group)
            .count()
            > 1),
        "and a group has one category id, not two"
    );
}
