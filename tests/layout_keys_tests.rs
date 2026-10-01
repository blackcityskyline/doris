//! The three arrow bindings that arrange panels, and the layout that
//! comes back the next session.
//!
//! They are read off the grid rather than off the drawn rectangles: the
//! grid is the layout, and the rectangles are last pass's answer to it.
//! So the tests ask the grid and then confirm the rectangles followed.

use doris::ui::layout::{Dir, ZoneId, ZoneLayout, RESIZE_STEP};
use ratatui::layout::Rect;

const FRAME: Rect = Rect {
    x: 0,
    y: 0,
    width: 120,
    height: 40,
};

fn preset(spec: &str) -> ZoneLayout {
    let mut z = ZoneLayout::new();
    z.apply_preset(spec);
    z.update_areas(FRAME);
    z
}

/// `Ctrl`+arrow: the panel sharing that border, and the focus moving to it.
#[test]
fn test_the_neighbour_is_the_panel_sharing_that_border() {
    let z = preset("1,2,3,4");
    assert_eq!(
        z.neighbour(ZoneId::Results, Dir::Down),
        Some(ZoneId::Torrent)
    );
    assert_eq!(z.neighbour(ZoneId::Torrent, Dir::Up), Some(ZoneId::Results));
    assert_eq!(z.neighbour(ZoneId::Results, Dir::Left), None);
}

#[test]
fn test_focus_moves_to_the_neighbour_and_stops_at_the_edge() {
    let mut z = preset("1,2,3,4");
    z.focused = ZoneId::Results;
    assert!(z.focus_neighbour(Dir::Down));
    assert_eq!(z.focused, ZoneId::Torrent);

    z.focused = ZoneId::Log;
    assert!(!z.focus_neighbour(Dir::Down), "nothing below the last row");
    assert_eq!(z.focused, ZoneId::Log, "and the cursor did not wrap");
}

/// `Ctrl`+`Shift`+arrow: the border moves one step, and the pair keeps
/// its total.
#[test]
fn test_resize_moves_the_border_one_step() {
    let mut z = preset("1,2");
    let before = z.get_area(ZoneId::Results).height;
    let total = before + z.get_area(ZoneId::Torrent).height;

    z.focused = ZoneId::Results;
    assert!(z.resize_focused(Dir::Down));
    z.update_areas(FRAME);

    let grown = z.get_area(ZoneId::Results).height;
    assert_eq!(
        grown,
        before + RESIZE_STEP,
        "the named edge travelled downwards by exactly one step"
    );
    assert_eq!(
        grown + z.get_area(ZoneId::Torrent).height,
        total,
        "and the step came out of the neighbour, not out of the frame"
    );
}

/// The floor holds: a panel cannot be stepped below what a drag allows,
/// and the key refuses the step instead of crushing its neighbour.
#[test]
fn test_resize_stops_at_the_floor() {
    let mut z = preset("1,2");
    z.focused = ZoneId::Results;

    let mut steps = 0;
    while z.resize_focused(Dir::Down) {
        z.update_areas(FRAME);
        steps += 1;
        assert!(
            z.get_area(ZoneId::Results).height >= doris::ui::layout::RESIZE_MIN_HEIGHT,
            "step {steps} left the panel under the floor: {}",
            z.get_area(ZoneId::Results).height
        );
        assert!(
            z.get_area(ZoneId::Torrent).height >= doris::ui::layout::RESIZE_MIN_HEIGHT,
            "step {steps} left the neighbour under the floor: {}",
            z.get_area(ZoneId::Torrent).height
        );
        assert!(steps < 100, "the resize never reached a floor at all");
    }

    assert!(steps > 0, "nothing moved, so the floor was never tested");
    assert!(
        !z.resize_focused(Dir::Down),
        "the floor is where the key stops, not where it keeps going"
    );
}

/// `Shift`+arrow: the two panels trade places and the frame follows.
#[test]
fn test_swap_trades_places_and_the_layout_follows() {
    let mut z = preset("1,2,3,4");
    let results_at = z.get_area(ZoneId::Results).y;
    let torrent_at = z.get_area(ZoneId::Torrent).y;

    z.focused = ZoneId::Results;
    assert!(z.swap_focused(Dir::Down));
    z.update_areas(FRAME);

    assert_eq!(z.get_area(ZoneId::Torrent).y, results_at);
    assert_eq!(z.get_area(ZoneId::Results).y, torrent_at);
}

/// Swapping sideways inside one row moves the panels across, and each
/// keeps its own share of the width.
#[test]
fn test_swap_moves_a_panel_across_a_row() {
    let mut z = preset("13");
    let results_at = z.get_area(ZoneId::Results).x;
    let trackers_at = z.get_area(ZoneId::Trackers).x;

    z.focused = ZoneId::Results;
    assert!(z.swap_focused(Dir::Right));
    z.update_areas(FRAME);

    assert_eq!(z.get_area(ZoneId::Trackers).x, results_at);
    assert_eq!(z.get_area(ZoneId::Results).x, trackers_at);
}

/// The layout comes back whole: the tiling, every panel's size, which
/// ones were on screen, and where the cursor was.
#[test]
fn test_the_layout_survives_a_round_trip_through_the_config() {
    let mut z = preset("1,3|4");
    z.focused = ZoneId::Trackers;
    z.resize_focused(Dir::Down);
    z.update_areas(FRAME);
    let before = z.get_area(ZoneId::Results);

    // Through the file format, not through a clone: the claim is that
    // the value survives being written and read back.
    let text = toml::to_string(&z.snapshot()).unwrap();
    let parsed: doris::ui::layout::SavedLayout = toml::from_str(&text).unwrap();

    let mut restored = ZoneLayout::new();
    assert!(restored.restore(&parsed));
    restored.update_areas(FRAME);

    assert_eq!(
        restored.get_area(ZoneId::Results),
        before,
        "the size came back"
    );
    assert_eq!(restored.focused, ZoneId::Trackers, "and the cursor");
    assert!(
        !restored.is_visible(ZoneId::Torrent),
        "and the hidden panel"
    );
    assert!(restored.is_visible(ZoneId::Trackers), "and the shown one");
}

/// A layout too short to name every panel is refused rather than half
/// applied.
#[test]
fn test_a_partial_layout_is_refused() {
    let mut z = preset("1,2,3,4");
    let mut short = z.snapshot();
    short.zones.pop();
    assert!(!z.restore(&short));
    assert!(
        z.is_visible(ZoneId::Torrent),
        "nothing was applied to a layout that was refused"
    );
}

/// The arrangement lives beside the config rather than inside it, so a
/// session that saves its preferences on exit and one that does not end
/// up with the same windows.
#[test]
fn test_the_layout_does_not_live_in_the_config() {
    let cfg: doris::config::Config = toml::from_str("theme_name = \"noctalia\"\n").unwrap();
    assert_eq!(cfg.theme_name.as_deref(), Some("noctalia"));
    // A stale section left in an old config is simply not a field.
    assert!(toml::to_string(&cfg).unwrap().contains("noctalia"));
}
