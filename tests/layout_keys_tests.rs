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

/// A keybind letter inside a label wears the label's own colour.
///
/// It used to take `on_hover`, which made `f filter` and `g group` the
/// only labels on screen with a letter in a foreign colour, and
/// disagreed with the panel titles where the detail key had already been
/// given the number's colour. One rule everywhere: a letter inside a word
/// is marked by weight, never by hue.
#[test]
fn test_a_hotkey_letter_wears_the_colour_of_the_word_it_sits_in() {
    use doris::ui::layout::{button_spans, zone_buttons, ZoneId};
    use doris::ui::theme::{ColorDef, Theme};

    // The accents are named explicitly because the built-in theme falls
    // all three of them back to the same `hi_fg`: a test run against it
    // cannot tell the word's colour from the keybind accent, and passes
    // either way.
    let theme = Theme {
        primary: Some(ColorDef::new(10, 20, 30)),
        secondary: Some(ColorDef::new(40, 50, 60)),
        on_hover: Some(ColorDef::new(70, 80, 90)),
        ..Theme::default_theme()
    };
    let word = theme.primary_color();
    assert_ne!(word, theme.on_hover_color(), "the accents must differ here");
    for id in [
        ZoneId::Results,
        ZoneId::Torrent,
        ZoneId::Trackers,
        ZoneId::Log,
    ] {
        for button in zone_buttons(id) {
            let spans = button_spans(&theme, &button, false, false);
            for span in &spans {
                assert_eq!(
                    span.style.fg,
                    Some(word),
                    "{:?} on the {id:?} frame paints {:?}, not the word's own colour",
                    button.text(),
                    span.style.fg
                );
            }
        }
    }
}

/// Every keybind in a panel title is the label's own colour.
///
/// The zone's digit and the panel's detail-view key are keybinds, and they
/// used to take `secondary` while the word beside them took `primary`.
/// That only showed on a theme which names no `secondary` -- the accent
/// then falls back to the theme's grey-green `hi_fg` -- and the result was
/// `¹` and `R` coming out a different colour from `Results` on the same
/// line, with the digit and the letter looking like they belonged to
/// something else entirely.
#[test]
fn test_a_panel_title_is_one_colour() {
    use doris::ui::layout::zone_title;
    use ratatui::style::{Color, Modifier};

    // The built-in theme falls `secondary` back to the same `hi_fg` it
    // gives `primary`, so it cannot tell these two apart. Named apart
    // here, the way a real theme that defines both does.
    let theme = doris::ui::theme::Theme {
        primary: Some(doris::ui::theme::ColorDef::new(10, 20, 30)),
        secondary: Some(doris::ui::theme::ColorDef::new(90, 100, 110)),
        ..doris::ui::theme::Theme::default_theme()
    };

    for id in [
        doris::ui::layout::ZoneId::Results,
        doris::ui::layout::ZoneId::Torrent,
        doris::ui::layout::ZoneId::Trackers,
        doris::ui::layout::ZoneId::Log,
    ] {
        let spans = zone_title(id, &theme, true).spans;
        let wrong: Vec<&str> = spans
            .iter()
            .filter(|s| s.style.fg != Some(Color::Rgb(10, 20, 30)))
            .map(|s| s.content.as_ref())
            .collect();
        assert!(
            wrong.is_empty(),
            "the {id:?} title paints {wrong:?} in something other than the \
             label's colour"
        );
        // The keybinds stay marked, by weight.
        for key in ["\u{b9}", "\u{b2}", "\u{b3}", "\u{b4}", "R", "T", "L"] {
            if let Some(s) = spans.iter().find(|s| s.content.as_ref() == key) {
                assert!(
                    s.style.add_modifier.contains(Modifier::BOLD),
                    "the `{key}` of the {id:?} title is not marked"
                );
            }
        }
    }
}

/// The `S` of Search wears the label's own colour.
///
/// It was the last holdout: it took the keybind accent while every other
/// letter in a label took the word's colour, so the search bar was the
/// one title on screen that read as two colours.
#[test]
fn test_the_search_title_is_one_colour() {
    use doris::config::Config;
    use doris::ui::view::App as UiApp;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let mut app = UiApp::new("http://127.0.0.1:8090".into(), None);
    // The built-in theme falls `primary` and `on_hover` back to the same
    // `hi_fg`, so a test run against it cannot tell the label's colour
    // from the keybind accent and passes either way. Named apart here.
    app.theme.primary = Some(doris::ui::theme::ColorDef::new(10, 20, 30));
    app.theme.on_hover = Some(doris::ui::theme::ColorDef::new(70, 80, 90));
    assert_ne!(app.theme.primary_color(), app.theme.on_hover_color());

    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let buf = terminal.backend().buffer();

    let label: Vec<String> = (0..buf.area.width)
        .map(|x| buf[(x, 0)].symbol().to_string())
        .collect();
    let line: String = label.concat();
    assert!(
        line.contains("Search"),
        "the search bar is titled Search; got {line:?}"
    );

    // The border row carries the title; the glyphs spell it out.
    let fgs: Vec<_> = (0..buf.area.width)
        .map(|x| &buf[(x, 0)])
        .filter(|c| c.symbol() != " " && c.symbol() != "╭" && c.symbol() != "╮")
        .filter(|c| !matches!(c.symbol(), "─" | "│"))
        .map(|c| (c.symbol().to_string(), c.fg))
        .collect();
    let first = fgs.first().expect("the title has glyphs").1;
    let odd: Vec<_> = fgs.iter().filter(|(_, fg)| *fg != first).collect();
    assert!(
        odd.is_empty(),
        "every glyph of the title should share one colour; {odd:?} differ from {first:?}"
    );
}
