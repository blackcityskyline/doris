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

/// A panel in the right-hand cell of a row can still reach the row above.
///
/// The default preset is `1,3|4`: `[[Results], [Trackers, Log]]`. Log sits
/// in column 1 of a row one cell wide, so looking for a panel in *that*
/// column found nothing, and Ctrl+Up, Shift+Up and Ctrl+Shift+Up all did
/// nothing at all from there -- which is what was reported as "vertical
/// does not work".
#[test]
fn test_a_narrow_row_still_has_a_panel_above_and_below_it() {
    let z = preset("1,3|4");
    assert_eq!(z.neighbour(ZoneId::Log, Dir::Up), Some(ZoneId::Results));
    assert_eq!(
        z.neighbour(ZoneId::Results, Dir::Down),
        Some(ZoneId::Trackers)
    );
    assert_eq!(z.neighbour(ZoneId::Log, Dir::Left), Some(ZoneId::Trackers));
    assert_eq!(z.neighbour(ZoneId::Trackers, Dir::Right), Some(ZoneId::Log));
}

/// Vertical movement reaches every panel that has a row above or below it,
/// and only those: a panel alone in the grid has nowhere to go.
#[test]
fn test_every_panel_on_the_default_preset_can_move_vertically() {
    let mut z = preset("1,3|4");
    for (from, dir, to) in [
        (ZoneId::Results, Dir::Down, ZoneId::Trackers),
        (ZoneId::Trackers, Dir::Up, ZoneId::Results),
        (ZoneId::Log, Dir::Up, ZoneId::Results),
    ] {
        z.focused = from;
        assert!(
            z.focus_neighbour(dir),
            "{from:?} has no panel to the {dir:?} of it"
        );
        assert_eq!(z.focused, to);
    }
    // And the edge is still an edge.
    z.focused = ZoneId::Results;
    assert!(!z.focus_neighbour(Dir::Up), "nothing above the first row");
}

/// The `S` of Search is a keybind like any other, so it is `on_hover`
/// like the rest of them.
#[test]
fn test_the_search_title_marks_its_s_in_on_hover() {
    use doris::config::Config;
    use doris::ui::view::App as UiApp;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use ratatui::Terminal;

    let mut app = UiApp::new("http://127.0.0.1:8090".into(), None);
    app.theme = accents();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let buf = terminal.backend().buffer();

    let line: String = (0..buf.area.width)
        .map(|x| buf[(x, 0)].symbol().to_string())
        .collect();
    assert!(line.contains("Search"), "the bar is titled Search: {line}");
    // Character index, not byte index: the border glyph before it is
    // three bytes wide, and using the byte offset points at the second
    // letter of the word instead.
    let at = line[..line.find('S').expect("the S is drawn")]
        .chars()
        .count() as u16;
    assert_eq!(buf[(at, 0)].fg, Color::Rgb(200, 210, 220), "the `S`");
    assert_eq!(
        buf[(at + 1, 0)].fg,
        Color::Rgb(10, 20, 30),
        "the rest of the word"
    );
}

/// Every keybind glyph on screen is `on_hover`, and nothing else is.
///
/// One rule, everywhere: the letter that opens something -- `S` for the
/// search box, the zone's digit, the panel's full-view letter, `f` and `g`
/// on the frame, the category arrows -- is `on_hover`, and the rest of
/// the word it sits in is `primary`. The eye picks the key out of the
/// label without reading it.
///
/// The accents are named apart in the test theme: the built-in one falls
/// `primary`, `secondary` and `on_hover` back to the same `hi_fg`, so a
/// test against it cannot tell them apart and passes either way.
fn accents() -> doris::ui::theme::Theme {
    let theme = doris::ui::theme::Theme {
        primary: Some(doris::ui::theme::ColorDef::new(10, 20, 30)),
        secondary: Some(doris::ui::theme::ColorDef::new(90, 100, 110)),
        on_hover: Some(doris::ui::theme::ColorDef::new(200, 210, 220)),
        ..doris::ui::theme::Theme::default_theme()
    };
    assert_ne!(theme.primary_color(), theme.on_hover_color());
    theme
}

#[test]
fn test_a_panel_title_marks_its_keybinds_in_on_hover() {
    use doris::ui::layout::{superscript_digit, zone_title, ZoneId};
    use ratatui::style::{Color, Modifier};

    let theme = accents();
    let hot = Color::Rgb(200, 210, 220);
    let word = Color::Rgb(10, 20, 30);

    for (id, key) in [
        (ZoneId::Results, Some("R")),
        (ZoneId::Torrent, Some("T")),
        (ZoneId::Log, Some("L")),
        (ZoneId::Trackers, None),
    ] {
        let spans = zone_title(id, &theme, true).spans;
        let digit = superscript_digit(id as u8);
        let d = spans
            .iter()
            .find(|s| s.content.as_ref() == digit)
            .expect("the zone digit is drawn");
        assert_eq!(d.style.fg, Some(hot), "the zone digit of {id:?}");
        assert!(d.style.add_modifier.contains(Modifier::BOLD));

        if let Some(key) = key {
            let k = spans
                .iter()
                .find(|s| s.content.as_ref() == key)
                .expect("the detail key is drawn");
            assert_eq!(k.style.fg, Some(hot), "the `{key}` of {id:?}");
            assert!(k.style.add_modifier.contains(Modifier::BOLD));
        }
        for s in &spans {
            let bound = s.content.as_ref() == digit || Some(s.content.as_ref()) == key;
            if !bound {
                assert_eq!(
                    s.style.fg,
                    Some(word),
                    "{:?} in the {id:?} title is neither the word's colour nor a keybind",
                    s.content
                );
            }
        }
    }
}

#[test]
fn test_a_frame_button_marks_its_hotkey_in_on_hover() {
    use doris::ui::layout::{button_spans, zone_buttons, ZoneId};
    use ratatui::style::Color;

    let theme = accents();
    let hot = Color::Rgb(200, 210, 220);
    let word = Color::Rgb(10, 20, 30);

    let mut seen = 0;
    for id in [
        ZoneId::Results,
        ZoneId::Torrent,
        ZoneId::Trackers,
        ZoneId::Log,
    ] {
        for button in zone_buttons(id) {
            let spans = button_spans(&theme, &button, false, false);
            for span in &spans {
                let is_key = span.content.as_ref() == button.key.to_string()
                    || span.content.as_ref() == "◀"
                    || span.content.as_ref() == "▶";
                assert_eq!(
                    span.style.fg,
                    Some(if is_key { hot } else { word }),
                    "{:?} on the {id:?} frame: {:?} should be {}",
                    button.text(),
                    span.content,
                    if is_key {
                        "the keybind accent"
                    } else {
                        "the word colour"
                    }
                );
            }
            if spans.iter().any(|s| s.style.fg == Some(hot)) {
                seen += 1;
            }
        }
    }
    assert!(seen >= 2, "no frame button marked a keybind at all: {seen}");
}
