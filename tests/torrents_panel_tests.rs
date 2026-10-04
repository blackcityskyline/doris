use doris::ui::torrents_panel::{self, detail_budget, plan};
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
/// At every width, the two rules the table rests on: the row fits the width
/// it was given, and the name keeps at least its minimum.
///
/// Both were checked at three hand-picked widths and neither held at the
/// ones in between: columns were taken greedily and the name trimmed
/// afterwards, so past a certain width the name was the part that overflowed
/// and got cut -- the one column the ordering exists to protect.
#[test]
fn test_the_plan_always_fits_and_always_leaves_the_name_a_minimum() {
    for width in 14usize..=140 {
        let Some(plan) = plan(width) else {
            continue;
        };
        let total: usize = plan.iter().map(|(_, w)| w + 2).sum();
        assert!(
            total <= width,
            "at {width} the row is {total} wide: {plan:?}"
        );
        let name = plan.first().expect("a plan has a name");
        assert_eq!(name.0, "name", "the name is first, or nothing is elastic");
        assert!(
            name.1 >= 12,
            "at {width} the name got {} columns: {plan:?}",
            name.1
        );
    }
}

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
        let first = columns.first().expect("a plan is never empty");
        assert_eq!(first.0, "name", "at width {width} the name was dropped");
        assert!(first.1 >= 12, "at width {width} the name got {first:?}");
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
    assert_eq!(
        w.first(),
        Some(&"name"),
        "the name is first, as the reference has it"
    );
    assert!(
        w.len() > n.len(),
        "a narrow panel drops columns: {} vs {}",
        w.len(),
        n.len()
    );
    // The kept columns keep the reference's order, so a narrow panel is a
    // prefix of a wide one and never a reshuffle.
    assert_eq!(
        n,
        w.iter().take(n.len()).cloned().collect::<Vec<_>>(),
        "the narrow plan is not a prefix of the wide one: {n:?} vs {w:?}"
    );
    for key in n.iter().filter(|k| **k != "name") {
        assert!(w.contains(key), "{key:?} appeared out of nowhere");
        assert!(
            w.iter().position(|k| k == key) >= Some(1),
            "{key:?} moved in front of the name"
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
    let summary = torrents_panel::summary(&rows, Some(128_000_000_000), Some(true));
    let line = summary.one_line();
    assert!(!line.contains('\n'), "the summary is one row: {line:?}");
    assert!(summary.active.contains("2 torrents"), "{line}");
    assert!(
        summary.speeds.contains("5.0 MB/s"),
        "dl is totalled: {line}"
    );
    assert!(line.contains("free 119.2 GB"), "{line}");
    assert!(
        !line.contains("unreachable"),
        "a daemon that answered is not announced as down"
    );
    assert!(
        line.contains("connected"),
        "and it is said to be there: {line}"
    );
}

/// Before the first poll the daemon has not been asked yet, which is not
/// the same as having failed to answer. Saying "unreachable" then is the
/// panel claiming something it does not know.
#[test]
fn test_an_unasked_daemon_is_not_called_unreachable() {
    let line = torrents_panel::summary(&[], None, None).one_line();
    assert!(!line.contains("unreachable"), "{line}");
    assert!(!line.contains("connected"), "{line}");
}

/// A daemon that is not running is said so on the panel. The alternative is
/// a row of zeros and no way to tell it from a stalled download.
#[test]
fn test_an_unreachable_daemon_says_so() {
    let line = torrents_panel::summary(&[], None, Some(false)).one_line();
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

/// The `T` view's rows, divided three ways.
///
/// One pure function because two callers need the same answer -- the
/// renderer, and the pointer, which has to know which row separates the two
/// boxes before it can tell a drag on it from a click. Drawn one way and
/// grabbed another is a divider that cannot be pulled.
#[test]
fn test_the_full_frame_budget_divides_what_it_has_between_three_boxes() {
    let b = detail_budget(30, 120, 8, None);

    assert_eq!(
        b.sections_height, 4,
        "the three summary boxes are four rows"
    );
    assert_eq!(
        b.sections_height + b.downloads_height + b.facts_height,
        30,
        "and the three of them are the whole view: {b:?}"
    );
    assert_eq!(b.facts_height, 10, "eight facts and the frame they sit in");
    assert!(b.downloads_height > 10, "the table keeps the rest: {b:?}");
}

/// A split the user set wins over the automatic one, and stops at both ends:
/// the table keeps a box, and the facts box keeps one, so a drag past either
/// end does not eat the other panel.
#[test]
fn test_a_split_the_user_set_is_kept_within_what_the_view_can_give() {
    let room = detail_budget(30, 120, 8, None);
    let total = room.sections_height + room.downloads_height + room.facts_height;

    let asked_for_more = detail_budget(30, 120, 8, Some(999));
    assert_eq!(
        asked_for_more.downloads_height + asked_for_more.facts_height,
        total - 4,
        "the table cannot grow into the facts box: {asked_for_more:?}"
    );
    assert_eq!(
        asked_for_more.facts_height, 3,
        "which keeps one line of facts: {asked_for_more:?}"
    );

    let asked_for_nothing = detail_budget(30, 120, 8, Some(0));
    assert_eq!(
        asked_for_nothing.downloads_height, 3,
        "nor can it be pushed out of the view: {asked_for_nothing:?}"
    );
}

/// The split is transient: `None` is the automatic budget, and that is what
/// a fresh view starts from.
#[test]
fn test_no_split_is_the_automatic_budget() {
    assert_eq!(
        detail_budget(30, 120, 8, None),
        detail_budget(30, 120, 8, None),
        "the same view twice is the same budget"
    );
    assert_ne!(
        detail_budget(30, 120, 8, None).downloads_height,
        detail_budget(30, 120, 8, Some(6)).downloads_height,
        "and a split is the only thing that changes it"
    );
}
