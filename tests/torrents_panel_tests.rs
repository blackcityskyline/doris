use doris::ui::torrents_panel::{self, plan};
use doris::ui::view::DownloadRow;

/// The panel is asked "what fits?" on every resize and every terminal, so
/// the answer is a pure function rather than something the renderer works
/// out while drawing. These are the widths that decide it.
fn row(name: &str) -> DownloadRow {
    DownloadRow {
        id: 1,
        hash: "045e85f2ebc24a875a64fe2e9ac9b61f7aad0499".into(),
        name: name.into(),
        fraction: 0.42,
        download_speed: 4_200_000,
        upload_speed: 890_000,
        seeds: 3,
        peers: 12,
        total_size: 1_990_000_000,
        left: 1_150_000_000,
        added: 1_700_000_000,
        dir: "/home/u/Downloads".into(),
        status: 4,
        uploaded: 340_000_000,
        downloaded: 840_000_000,
        eta: 359,
        ..DownloadRow::default()
    }
}

/// The rule the whole table rests on: the name is never dropped. A row that
/// cannot say what it is about is not a row, and a panel full of
/// percentages under no headings is worse than a narrower table.
#[test]
fn test_the_name_survives_at_every_width_a_panel_can_have() {
    for width in 14..=200 {
        let Some(columns) = plan(width) else {
            assert!(
                width < 14,
                "nothing fits at {width}, which should not happen above 13"
            );
            continue;
        };
        let last = columns.last().expect("a plan is never empty");
        assert_eq!(last.0, "name", "at width {width} the name was dropped");
        assert!(last.1 >= 12, "at width {width} the name got {last:?}");
    }
}

/// Narrower than a name and the panel says how many there are rather than
/// drawing a table of truncated nothing.
#[test]
fn test_a_panel_too_narrow_for_a_name_says_so_instead_of_drawing_one() {
    assert!(plan(13).is_none(), "twelve columns is not a table");
    assert!(plan(0).is_none());
    assert!(plan(1).is_none());
    assert!(plan(14).is_some());
}

/// Columns are dropped from the tail and the ones kept stay in one order,
/// so the table does not reshuffle itself as the window changes: a
/// percentage column that becomes a name column when the panel narrows is
/// unreadable.
#[test]
fn test_columns_disappear_from_the_tail_and_never_reordered() {
    let wide = plan(160).expect("wide");
    let narrow = plan(60).expect("narrow");

    let order =
        |columns: &[(&'static str, usize)]| columns.iter().map(|(k, _)| *k).collect::<Vec<_>>();
    let (w, n) = (order(&wide), order(&narrow));
    // No column may appear twice. A mutation that made the name take part
    // in the fitting loop instead of stopping it pushed `name` into the
    // plan a second time, and the table drew it twice -- which every other
    // assertion here was happy with.
    for (label, keys) in [("wide", &w), ("narrow", &n)] {
        let mut seen: Vec<&str> = Vec::new();
        for key in keys.iter() {
            assert!(
                !seen.contains(key),
                "{label}: {key:?} appears twice: {keys:?}"
            );
            seen.push(key);
        }
        assert_eq!(seen.len(), keys.len(), "{label}: {keys:?}");
    }
    assert_eq!(w.last(), Some(&"name"));
    assert!(
        w.len() > n.len(),
        "a narrow panel drops columns: {} vs {}",
        w.len(),
        n.len()
    );
    for key in &n {
        assert!(w.contains(key), "{key:?} appeared out of nowhere");
        assert!(
            w.iter().position(|k| k == key) <= w.iter().rposition(|k| k == &"name"),
            "{key:?} moved after the name"
        );
    }
}

/// Two rows differ only in their name and must not render identically:
/// that is the bug a truncated name produces when nothing is truncated.
#[test]
fn test_two_different_downloads_never_render_as_one_line() {
    let plan = plan(120).expect("a plan");
    let a = torrents_panel::row(&row("Oblivion (2013) [1080p]"), &plan);
    let b = torrents_panel::row(&row("Predator (1987)"), &plan);
    assert_ne!(a, b);
}

/// The header names the same columns, in the same order, as the rows.
#[test]
fn test_the_header_lines_up_with_the_rows() {
    let plan = plan(120).expect("a plan");
    let header = torrents_panel::header(&plan);
    let body = torrents_panel::row(&row("x"), &plan);
    // In characters, not bytes: the ellipsis that marks a shortened cell is
    // one column and three bytes, and a panel is measured in columns.
    assert_eq!(
        header.chars().count(),
        body.chars().count(),
        "header and body must be the same width"
    );
    for (key, _) in &plan {
        let title = doris::ui::torrents_panel::COLUMNS
            .iter()
            .find(|c| c.key == *key)
            .map(|c| c.title)
            .unwrap();
        assert!(header.contains(title), "the header does not name {key:?}");
    }
}

/// The summary line is what the panel is asked most often -- how much is
/// moving and how many things are being fetched -- so it carries the totals
/// and stays one line whatever is in the list.
#[test]
fn test_the_summary_totals_the_list_and_stays_one_line() {
    let rows = vec![
        row("one"),
        DownloadRow {
            download_speed: 1_000_000,
            uploaded: 10,
            downloaded: 100,
            ..row("two")
        },
    ];
    let line = torrents_panel::stats(&rows, Some(128_000_000_000), true);
    assert!(!line.contains('\n'), "the summary is one row: {line:?}");
    assert!(line.contains("2 torrents"), "{line}");
    assert!(line.contains("5.0 MB/s"), "dl is totalled: {line}");
    assert!(line.contains("free 119.2 GB"), "{line}");
    assert!(
        !line.contains("unreachable"),
        "a daemon that answered is not announced as down"
    );
}

/// A daemon that is not running is said so on the panel. The alternative is
/// a row of zeros and no way to tell it from a stalled download.
#[test]
fn test_an_unreachable_daemon_says_so() {
    let line = torrents_panel::stats(&[], None, false);
    assert!(line.contains("[daemon unreachable]"), "{line}");
    assert!(line.contains("0 torrents"), "{line}");
}

/// A zero speed is `0`, not `0.0 B/s`: a column of zeros should not be a
/// column of noise, and every column being empty is what makes a paused
/// table unreadable.
#[test]
fn test_a_zero_speed_is_a_zero_and_not_a_unit() {
    assert_eq!(doris::transmission::human_speed(0), "0");
    assert_eq!(doris::transmission::human_speed(4_200_000), "4.0 MB/s");
    assert_eq!(doris::transmission::human_speed(890), "890 B/s");
}

/// A ratio before anything is downloaded prints as `--`, not `0.00`:
/// `0.00` is a measurement and there has not been one.
#[test]
fn test_an_unmeasured_ratio_prints_as_two_dashes() {
    let plan = plan(120).expect("a plan");
    let mut fresh = row("fresh");
    fresh.downloaded = 0;
    assert!(torrents_panel::row(&fresh, &plan).contains("--"));
    fresh.downloaded = 800;
    fresh.uploaded = 200;
    assert!(torrents_panel::row(&fresh, &plan).contains("0.25"));
}
