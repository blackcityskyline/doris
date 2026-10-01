//! Hovering a frame button. The buttons on a panel's frame (`f filter`, `p pause`, `d delete`,
//! the category arrows) are clickable and looked like words.

use doris::ui::layout::ZoneId;
use doris::ui::view::App;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::Terminal;

/// An app laid out at a size wide enough for every frame button, which
/// is the only way to look at them: `frame_layout` drops a button that does
/// not fit rather than clipping it, so a zero-sized (never-rendered) zone
/// has no buttons at all.
fn app() -> App {
    let mut app = App::new("http://127.0.0.1:1".into(), None);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|f| app.render(f, &doris::config::Config::default()))
        .expect("a first frame");
    app
}

/// Where a zone's first frame button is drawn, from the same layout the renderer and `click_at`
/// both read.
fn first_button(app: &App, id: ZoneId) -> Rect {
    let area = app.zones.get_area(id);
    let layout = app.frame_layout(id, area, &doris::config::Config::default());
    layout
        .buttons
        .first()
        .map(|(_, r)| *r)
        .unwrap_or_else(|| panic!("{id:?} has no frame buttons"))
}

#[test]
fn a_button_lights_up_when_the_pointer_is_on_it() {
    let mut app = app();
    let rect = first_button(&app, ZoneId::Results);
    assert!(!app.hovers(rect), "nothing is hovered to begin with");

    assert!(
        app.set_hover(rect.y, rect.x),
        "moving onto a button is a change worth redrawing for"
    );
    assert!(app.hovers(rect));

    // Every cell of the button counts: the label is three cells wide and
    for col in rect.x..rect.x + rect.width {
        assert!(
            app.hovers(rect),
            "cell {col} of the button must count as on it"
        );
    }
}

#[test]
fn a_button_goes_out_when_the_pointer_leaves_it() {
    let mut app = app();
    let rect = first_button(&app, ZoneId::Results);
    app.set_hover(rect.y, rect.x);

    app.set_hover(rect.y, rect.x + rect.width);
    assert!(
        !app.hovers(rect),
        "one cell past the button is off it, not a near miss"
    );
}

#[test]
fn moving_within_one_cell_is_not_a_change() {
    let mut app = app();
    app.set_hover(30, 30);
    assert!(app.set_hover(3, 7));
    assert!(
        !app.set_hover(3, 7),
        "the terminal reports every movement; an unchanged cell must not \
         cost a redraw"
    );
    assert!(app.set_hover(3, 8));
}

/// The hover rectangle and the click rectangle are the same rectangle.
#[test]
fn what_lights_up_is_what_a_click_hits() {
    let mut app = app();
    let config = doris::config::Config::default();
    let mut checked = 0;

    for &id in ZoneId::all() {
        let area = app.zones.get_area(id);
        let layout = app.frame_layout(id, area, &config);
        for (button, rect) in &layout.buttons {
            // Every cell of the button, not just the middle: the edges
            for col in rect.x..rect.x + rect.width {
                let hit = layout.button_at(col, rect.y);
                assert_eq!(
                    hit.as_ref().map(|(b, _)| b.label.clone()),
                    Some(button.label.clone()),
                    "{id:?}/{}: the cell at column {col} resolves elsewhere",
                    button.label
                );

                app.set_hover(rect.y, col);
                assert!(
                    app.hovers(*rect),
                    "{id:?}/{}: column {col} must light up",
                    button.label
                );
                checked += 1;
            }
        }
    }

    assert!(checked > 10, "only {checked} cells checked -- too few");
}

/// The render actually asks.
#[test]
fn the_drawn_frame_marks_the_hovered_button() {
    let mut app = app();
    let rect = first_button(&app, ZoneId::Results);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    let config = doris::config::Config::default();

    let underlined_at = |term: &Terminal<TestBackend>, rect: Rect| -> usize {
        let buf = term.backend().buffer().clone();
        (rect.x..rect.x + rect.width)
            .filter(|&c| buf[(c, rect.y)].modifier.contains(Modifier::UNDERLINED))
            .count()
    };

    terminal.draw(|f| app.render(f, &config)).unwrap();
    let at_rest = underlined_at(&terminal, rect);
    assert_eq!(at_rest, 0, "nothing is hovered to begin with");

    app.set_hover(rect.y, rect.x);
    terminal.draw(|f| app.render(f, &config)).unwrap();
    let hovered = underlined_at(&terminal, rect);
    assert_eq!(
        hovered, rect.width as usize,
        "the whole button is marked, not one cell of it"
    );
}

/// A hovered button is marked by more than its colour: a theme whose
/// `on_hover` sits close to `primary` would otherwise leave the pointer's
/// position unreadable, which is the same one-channel problem the zone
/// focus marker solves.
#[test]
fn a_hovered_button_is_underlined_not_merely_tinted() {
    use doris::ui::theme::Theme;
    let theme = Theme::default();
    let button = doris::ui::layout::FrameButton {
        slot: doris::ui::layout::FrameSlot::TopLeft,
        key: 'f',
        label: "filter".to_string(),
    };

    let plain = doris::ui::layout::button_spans(&theme, &button, false, false);
    let hovered = doris::ui::layout::button_spans(&theme, &button, false, true);

    let underlined = |spans: &[ratatui::text::Span]| {
        spans
            .iter()
            .any(|s| s.style.add_modifier.contains(Modifier::UNDERLINED))
    };
    assert!(!underlined(&plain), "at rest it is not underlined");
    assert!(underlined(&hovered), "hovered it is");
}
