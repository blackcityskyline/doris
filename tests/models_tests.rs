use doris::search::format::parse_size;
use doris::search::models::*;

#[test]
fn test_torrent_item_full_json() {
    let json = r#"{
        "title": "Ubuntu 24.04 LTS",
        "size": "4.2 GB",
        "seeds": "150",
        "download_url": "/forum/dl.php?t=12345",
        "query": "ubuntu",
        "date": "01-Янв-26",
        "page_url": "viewtopic.php?t=12345"
    }"#;
    let item: TorrentItem = serde_json::from_str(json).unwrap();
    assert_eq!(item.title, "Ubuntu 24.04 LTS");
    assert_eq!(item.size, "4.2 GB");
    assert_eq!(item.seeds, "150");
    assert_eq!(item.download_url, "/forum/dl.php?t=12345");
    assert_eq!(item.date, "01-Янв-26");
}

#[test]
fn test_torrent_item_minimal_json() {
    let json = r#"{"title": "Test"}"#;
    let item: TorrentItem = serde_json::from_str(json).unwrap();
    assert_eq!(item.title, "Test");
    assert_eq!(item.size, "");
    assert_eq!(item.seeds, "");
    assert_eq!(item.download_url, "");
    assert_eq!(item.date, "");
    assert_eq!(item.page_url, "");
}

#[test]
fn test_torrent_item_vec_deserialize() {
    let json = r#"[
        {"title": "First", "seeds": "10", "size": "1 GB"},
        {"title": "Second", "seeds": "5", "size": "2 GB"}
    ]"#;
    let items: Vec<TorrentItem> = serde_json::from_str(json).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].title, "First");
    assert_eq!(items[1].title, "Second");
}

#[test]
fn test_torrent_item_empty_array() {
    let json = "[]";
    let items: Vec<TorrentItem> = serde_json::from_str(json).unwrap();
    assert_eq!(items.len(), 0);
}

#[test]
fn test_torrent_item_source_field_defaults_to_empty_and_deserializes_when_present() {
    let without_source: TorrentItem = serde_json::from_str(r#"{"title": "Test"}"#).unwrap();
    assert_eq!(without_source.source, "");

    let with_source: TorrentItem = serde_json::from_str(r#"{"title": "Test", "source": "rutor"}"#).unwrap();
    assert_eq!(with_source.source, "rutor");
}

// --- parse_size (B1: Rust port of torio's util/format.ts parseSize) --------

#[test]
fn test_parse_size_latin_binary_units() {
    assert_eq!(parse_size("1.45 GiB"), 1_556_925_645);
    assert_eq!(parse_size("856 MiB"), 897_581_056);
    assert_eq!(parse_size("500 KiB"), 512_000);
}

#[test]
fn test_parse_size_latin_decimal_units() {
    // KB/MB/GB/TB are SI (1000-based) -- the same asymmetry torio has.
    assert_eq!(parse_size("4.2 GB"), 4_200_000_000);
    assert_eq!(parse_size("528.42 MB"), 528_420_000);
    assert_eq!(parse_size("0 B"), 0);
}

#[test]
fn test_parse_size_russian_units_are_binary() {
    // `ГБ`/`МБ`/`КБ` are read as GiB/MiB/KiB, and both the comma decimal
    // separator rutor serves and the lower-case spelling must work.
    assert_eq!(parse_size("2,27 ГБ"), 2_437_393_940);
    assert_eq!(parse_size("2.27 ГБ"), 2_437_393_940);
    assert_eq!(parse_size("750 мб"), 786_432_000);
}

#[test]
fn test_parse_size_raw_byte_string() {
    // No unit at all: the digits are already bytes.
    assert_eq!(parse_size("12345678"), 12_345_678);
}

#[test]
fn test_parse_size_garbage_is_zero() {
    assert_eq!(parse_size(""), 0);
    assert_eq!(parse_size("abc"), 0);
    assert_eq!(parse_size("GB"), 0);
    assert_eq!(parse_size("12.5"), 0); // fractional, so not a raw count
    assert_eq!(parse_size("size unknown"), 0);
}

#[test]
fn test_parse_size_stops_at_the_second_dot_like_js_parse_float() {
    // Parity with torio: `parseFloat("2.27.5")` yields 2.27 rather than
    // failing the way Rust's `str::parse::<f64>` would.
    assert_eq!(parse_size("2.27.5 GB"), 2_270_000_000);
}

// --- TorrentItem v2 fields (B1) --------------------------------------------

#[test]
fn test_b1_fields_default_when_absent_from_json() {
    // Rutracker's browser-eval script only sets the display fields, so
    // every B1 field has to deserialize from nothing rather than fail.
    let item: TorrentItem = serde_json::from_str(r#"{"title": "Test"}"#).unwrap();
    assert_eq!(item.group, None);
    assert_eq!(item.info_hash, "");
    assert_eq!(item.magnet, None);
    assert_eq!(item.size_bytes, 0);
    assert_eq!(item.seeds_n, 0);
    assert_eq!(item.leechers, 0);
    assert_eq!(item.added, 0);
}

#[test]
fn test_b1_fields_deserialize_when_present() {
    let json = r#"{
        "title": "Some Torrent",
        "group": "Movies",
        "info_hash": "06555d165746e815b0ab5b16de37ed24f9142595",
        "magnet": "magnet:?xt=urn:btih:06555d165746e815b0ab5b16de37ed24f9142595",
        "size_bytes": 4521000000,
        "seeds_n": 42,
        "leechers": 7,
        "added": 1788652800
    }"#;
    let item: TorrentItem = serde_json::from_str(json).unwrap();
    assert_eq!(item.group, Some(doris::search::source::Group::Movies));
    assert_eq!(item.info_hash, "06555d165746e815b0ab5b16de37ed24f9142595");
    assert!(item.magnet.as_deref().unwrap().starts_with("magnet:?xt=urn:btih:"));
    assert_eq!(item.size_bytes, 4_521_000_000);
    assert_eq!(item.seeds_n, 42);
    assert_eq!(item.leechers, 7);
    assert_eq!(item.added, 1_788_652_800);
}

#[test]
fn test_fill_from_display_derives_numeric_twins() {
    let mut item = TorrentItem {
        size: "2,27 ГБ".to_string(),
        seeds: " 42 ".to_string(),
        ..Default::default()
    };
    item.fill_from_display();
    assert_eq!(item.size_bytes, 2_437_393_940);
    assert_eq!(item.seeds_n, 42);
}

#[test]
fn test_fill_from_display_leaves_unparseable_values_at_zero() {
    let mut item = TorrentItem::default();
    item.fill_from_display();
    assert_eq!(item.size_bytes, 0);
    assert_eq!(item.seeds_n, 0);
    assert_eq!(item.added, 0, "fill_from_display must not touch fields it derives from");
}

// --- format_bytes / format_date (B8 wave 1: JSON sources report numbers) ----

#[test]
fn test_format_bytes_matches_torios_format_bytes() {
    use doris::search::format::format_bytes;

    assert_eq!(format_bytes(0), "0 B", "unknown size must still look like a size");
    assert_eq!(format_bytes(511), "511 B");
    assert_eq!(format_bytes(1024), "1.00 KB");
    // torio steps by 1024 but labels the units SI -- kept identical so a
    // YTS row reads the same as a row a tracker rendered for us.
    assert_eq!(format_bytes(511_568_773), "487.87 MB");
    assert_eq!(format_bytes(1_073_741_824), "1.00 GB");
    // Just under a unit boundary: two decimals round up rather than
    // silently promoting the row to the next unit.
    assert_eq!(format_bytes(1_073_741_224), "1024.00 MB");
    assert_eq!(format_bytes(4_000_000_000), "3.73 GB");
}

#[test]
fn test_format_date_renders_utc_and_refuses_the_epoch() {
    use doris::search::format::format_date;

    assert_eq!(format_date(1_705_959_944), "2024-01-22");
    assert_eq!(format_date(0), "", "the zero value means unknown, not 1970");
    assert_eq!(format_date(-5), "", "a negative timestamp is not a date");
}
