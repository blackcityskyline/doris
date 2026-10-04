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
fn test_the_row_fills_the_width_exactly_and_the_name_keeps_its_minimum() {
    for width in 14usize..=200 {
        let Some(plan) = plan(width) else {
            continue;
        };
        // Exactly, not "no more than": the panel used to reserve two columns
        // per column that the drawing never spent, which is where the empty
        // sixth of the screen on the right came from.
        let drawn = torrents_panel::row(&row("something"), &plan);
        assert_eq!(
            drawn.chars().count(),
            width,
            "at {width} the row is {} wide: {plan:?}",
            drawn.chars().count()
        );
        assert_eq!(
            torrents_panel::header(&plan).chars().count(),
            width,
            "at {width} the header does not match the row: {plan:?}"
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

/// The gaps say which columns answer the same question: one space inside a
/// group, three between groups.
///
/// `Name | Progress Status Size | Down Up | Seeds Peers Ratio`. A row that
/// spaces every column equally says the columns are equally related, and
/// they are not -- a name next to `Progress` is a different kind of thing
/// from `Down` next to `Up`.
///
/// Measured on the drawn header, between where one column's cell ends and
/// where the next one's title begins: every other column is right-aligned in
/// a cell of its own width, so its title ends exactly at that cell's right
/// edge and what is left between them is the gap and nothing else.
#[test]
fn test_the_columns_are_grouped_and_the_gaps_say_so() {
    let wide = plan(160).expect("a plan");
    let header = torrents_panel::header(&wide);
    let keys: Vec<&str> = wide.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        keys,
        vec!["name", "percent", "state", "size", "down", "up", "seeds", "peers", "ratio"],
        "the reference's order: {keys:?}"
    );

    let width_of = |key: &str| {
        wide.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, w)| *w)
            .unwrap_or_else(|| panic!("{key} is not in the plan"))
    };
    let title_of = |key: &str| {
        torrents_panel::COLUMNS
            .iter()
            .find(|c| c.key == key)
            .map(|c| c.title)
            .unwrap_or_else(|| panic!("{key} has no title"))
    };
    let at = |key: &str| {
        header
            .find(title_of(key))
            .unwrap_or_else(|| panic!("{key} is not in the header: {header:?}"))
    };
    // Where the cell of `key` ends: the name's cell is left-aligned and runs
    // from the left edge, and every other column's title is right-aligned, so
    // its last character sits in the cell's last one.
    let cell_end = |key: &str| {
        if key == "name" {
            width_of(key)
        } else {
            at(key) + title_of(key).chars().count()
        }
    };
    // The gap is what is between two *cells*. Measuring it from one title's
    // last character to the next title's first would count the second cell's
    // own left padding as well -- `Down` sits in a cell of ten and is four
    // characters wide -- so that padding comes off first.
    let leading_pad = |key: &str| width_of(key) - title_of(key).chars().count();
    let gap = |left: &str, right: &str| -> usize {
        let between = &header[cell_end(left)..at(right) - leading_pad(right)];
        assert_eq!(
            between.trim(),
            "",
            "what is between {left} and {right} should be only the gap: {between:?}"
        );
        between.chars().count()
    };

    for (left, right) in [("name", "percent"), ("size", "down"), ("up", "seeds")] {
        assert_eq!(
            gap(left, right),
            3,
            "{left} and {right} are different groups, so three columns of gap"
        );
    }
    for (left, right) in [
        ("percent", "state"),
        ("state", "size"),
        ("down", "up"),
        ("seeds", "peers"),
        ("peers", "ratio"),
    ] {
        assert_eq!(
            gap(left, right),
            1,
            "{left} and {right} answer the same question, so one column of gap"
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
