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
fn test_the_search_title_marks_its_s_in_hi_fg() {
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
    assert_eq!(buf[(at, 0)].fg, Color::Rgb(221, 188, 224), "the `S`");
    assert_eq!(
        buf[(at + 1, 0)].fg,
        Color::Rgb(171, 199, 255),
        "the rest of the word"
    );
}

/// Every keybind glyph on screen is `hi_fg`, and every other letter is
/// `title`.
///
/// This is the reference's split, verbatim: the letter that opens
/// something -- `S` for the search box, the zone's digit, the panel's
/// full-view letter, `f` and `g` on the frame, the category arrows --
/// takes `hi_fg`, and the rest of the word takes `title`
/// Both are mandatory in every theme file, so no
/// theme can name a keybind the colour of ordinary text and lose it,
/// which is exactly what an optional accent slot allowed: on one theme
/// the `S` of `Search` and the `f` of `filter` came out in `main_fg`.
///
/// The tokens are named apart in the test theme because the built-in one
/// makes `title` and `hi_fg` the same colour, so a test against it cannot
/// tell them apart and passes either way.
fn accents() -> doris::ui::theme::Theme {
    // The two tokens a label is drawn from, given values a screenshot
    // could tell apart: the word `title`, the keybind `hi_fg`.
    let theme = doris::ui::theme::Theme {
        title: doris::ui::theme::ColorDef::new(171, 199, 255),
        hi_fg: doris::ui::theme::ColorDef::new(221, 188, 224),
        primary: Some(doris::ui::theme::ColorDef::new(10, 20, 30)),
        secondary: Some(doris::ui::theme::ColorDef::new(90, 100, 110)),
        ..doris::ui::theme::Theme::default_theme()
    };
    assert_ne!(theme.title.to_color(), theme.hi_fg.to_color());
    theme
}

#[test]
fn test_a_panel_title_marks_its_keybinds_in_hi_fg() {
    use doris::ui::layout::{superscript_digit, zone_title, ZoneId};
    use ratatui::style::{Color, Modifier};

    let theme = accents();
    let hot = Color::Rgb(221, 188, 224);
    let word = Color::Rgb(171, 199, 255);

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
fn test_a_frame_button_marks_its_hotkey_in_hi_fg() {
    use doris::ui::layout::{button_spans, zone_buttons, ZoneId};
    use ratatui::style::Color;

    let theme = accents();
    let hot = Color::Rgb(221, 188, 224);
    let word = Color::Rgb(171, 199, 255);

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

/// The arrow names the focused panel's own edge, so that panel is the one
/// that grows -- whichever side it sits on.
///
/// This is the rule that was wrong: the side that grew was decided by the
/// direction, so `Left` grew the *other* panel, which is the same as
/// shrinking the focused one. A run showed it as: the left panel only ever
/// growing, the right panel only ever shrinking, `Left` doing nothing from
/// the leftmost panel, and `Right` doing nothing from the rightmost.
///
#[test]
fn test_the_focused_panel_grows_towards_the_arrow() {
    // `12` is two cells of one row (side by side); `1,2` is two rows.
    let cases: [(ZoneId, &str, Dir); 4] = [
        (ZoneId::Results, "12", Dir::Right),
        (ZoneId::Torrent, "12", Dir::Left),
        (ZoneId::Results, "1,2", Dir::Down),
        (ZoneId::Torrent, "1,2", Dir::Up),
    ];
    let mut grew = 0;

    for (focus, spec, dir) in cases {
        {
            let mut z = preset(spec);
            z.focused = focus;
            z.update_areas(FRAME);
            let before = measure(z.get_area(focus), dir);
            let had_neighbour = z.neighbour(focus, dir).is_some();
            let moved = z.resize_focused(dir);
            z.update_areas(FRAME);
            let after = measure(z.get_area(focus), dir);

            if had_neighbour {
                assert!(moved, "{focus:?} could not grow towards {dir:?} on {spec}");
                assert!(
                    after > before,
                    "{focus:?} towards {dir:?} on {spec}: {before} -> {after}, so the \
                     arrow shrank the panel it names"
                );
                grew += 1;
            } else {
                // Nothing that way, but the other side has a neighbour,
                // so the panel still moves -- the other way.
                assert!(
                    moved,
                    "{focus:?} did nothing on {dir:?}, and it has a neighbour"
                );
                assert_ne!(
                    after, before,
                    "{focus:?} moved on {dir:?} without changing size"
                );
            }
        }
    }
    assert_eq!(grew, cases.len(), "every case had a neighbour and moved");
}

/// The measured side: the one the arrow names.
fn measure(r: Rect, dir: Dir) -> u32 {
    match dir {
        Dir::Left | Dir::Right => r.width as u32,
        Dir::Up | Dir::Down => r.height as u32,
    }
}

/// An arrow naming the frame's own edge cannot grow the panel that way --
/// there is nothing on the other side of it to take the space from -- so
/// the panel shrinks towards its other neighbour instead.
///
/// The older answer was to refuse, and that read as a dead key: from the
/// left panel `Left` did nothing, from the right panel `Right` did
/// nothing, and from a full-width panel both did nothing. Every arrow a
/// panel has a neighbour for now moves the panel.
#[test]
fn test_an_arrow_at_the_frame_edge_shrinks_the_panel() {
    for (spec, focus, dir, other) in [
        ("12", ZoneId::Results, Dir::Left, ZoneId::Torrent),
        ("12", ZoneId::Torrent, Dir::Right, ZoneId::Results),
    ] {
        let mut z = preset(spec);
        z.focused = focus;
        z.update_areas(FRAME);
        let mine = z.get_area(focus);
        let theirs = z.get_area(other);
        assert!(
            z.resize_focused(dir),
            "{focus:?} did not move on {dir:?} with nothing beyond it"
        );
        z.update_areas(FRAME);
        assert!(
            z.get_area(focus).width < mine.width,
            "{focus:?} towards {dir:?}: {} -> {}, so it grew into the frame edge",
            mine.width,
            z.get_area(focus).width
        );
        assert!(
            z.get_area(other).width > theirs.width,
            "{other:?} did not get the space"
        );
    }
}

/// Down the rows, the same rule.
#[test]
fn test_an_arrow_at_the_top_edge_shrinks_the_row() {
    let mut z = preset("1,2");
    z.focused = ZoneId::Results;
    z.update_areas(FRAME);
    let mine = z.get_area(ZoneId::Results);
    assert!(
        z.resize_focused(Dir::Up),
        "the top panel did not move on Up"
    );
    z.update_areas(FRAME);
    assert!(
        z.get_area(ZoneId::Results).height < mine.height,
        "the top panel grew upwards into the frame edge"
    );
}

/// One panel alone in the frame has no neighbour on any side, so every
/// arrow is dead -- and that is the only case where one is.
#[test]
fn test_a_panel_alone_in_the_frame_has_a_dead_arrow() {
    let mut z = preset("1");
    z.focused = ZoneId::Results;
    z.update_areas(FRAME);
    let before = z.get_area(ZoneId::Results);
    for dir in [Dir::Left, Dir::Right, Dir::Up, Dir::Down] {
        assert!(
            !z.resize_focused(dir),
            "{dir:?} moved a panel with no neighbour"
        );
    }
    z.update_areas(FRAME);
    assert_eq!(z.get_area(ZoneId::Results), before);
}

/// Every arrow on every panel of a two-cell row moves that panel.
#[test]
fn test_every_arrow_moves_the_focused_panel() {
    for focus in [ZoneId::Results, ZoneId::Torrent] {
        for dir in [Dir::Left, Dir::Right] {
            let mut z = preset("12");
            z.focused = focus;
            z.update_areas(FRAME);
            let before = z.get_area(focus).width;
            assert!(z.resize_focused(dir), "{focus:?} {dir:?} did nothing");
            z.update_areas(FRAME);
            assert_ne!(
                z.get_area(focus).width,
                before,
                "{focus:?} {dir:?} moved but not its own width"
            );
        }
    }
}

/// The neighbour gives up exactly what the focused panel takes.
#[test]
fn test_the_border_moves_and_the_pair_still_fills_the_frame() {
    let mut z = preset("12");
    z.focused = ZoneId::Results;
    z.update_areas(FRAME);

    assert!(z.resize_focused(Dir::Right));
    z.update_areas(FRAME);
    let mine = z.get_area(ZoneId::Results);
    let theirs = z.get_area(ZoneId::Torrent);
    assert!(
        mine.width > FRAME.width / 2,
        "the left panel did not grow: {mine:?}"
    );
    assert!(
        theirs.width < FRAME.width / 2,
        "the right one did not give: {theirs:?}"
    );
    assert_eq!(
        mine.width + theirs.width,
        FRAME.width,
        "the pair still fills the frame"
    );
}

/// Pressed only on one side, each panel grows the way its own key says.
///
/// Equal numbers of presses on both sides land back where they started, so
/// each side is driven alone here. This is the whole of the report: from
/// the right panel, `Left` used to shrink it instead of growing it.
#[test]
fn test_each_panel_grows_on_its_own_side() {
    for (focus, dir, other) in [
        (ZoneId::Results, Dir::Right, ZoneId::Torrent),
        (ZoneId::Torrent, Dir::Left, ZoneId::Results),
    ] {
        let mut z = preset("12");
        let start = z.get_area(focus).width;
        for _ in 0..3 {
            z.focused = focus;
            z.update_areas(FRAME);
            assert!(z.resize_focused(dir), "{focus:?} {dir:?}");
            z.update_areas(FRAME);
        }
        assert!(
            z.get_area(focus).width > start,
            "{focus:?} pressed {dir:?} three times: {} -> {}",
            start,
            z.get_area(focus).width
        );
        assert!(
            z.get_area(other).width < start,
            "{other:?} did not give the space to {focus:?}"
        );
    }
}

/// The same, down the rows.
#[test]
fn test_each_row_panel_grows_on_its_own_side() {
    let mut z = preset("1,2");
    for _ in 0..3 {
        for (focus, dir) in [(ZoneId::Results, Dir::Down), (ZoneId::Torrent, Dir::Up)] {
            z.focused = focus;
            z.update_areas(FRAME);
            assert!(z.resize_focused(dir), "{focus:?} {dir:?}");
            z.update_areas(FRAME);
        }
    }
    let top = z.get_area(ZoneId::Results).height;
    let bottom = z.get_area(ZoneId::Torrent).height;
    assert!(
        top > bottom,
        "the top panel should be ahead: {top} / {bottom}"
    );
}
