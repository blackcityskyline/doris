//! The main menu is an overlay of glyphs, and nothing else.
//!
//! The reference builds the frame, prints it, and only then prints
//! `Global::overlay` on top of it (`btop.cpp:760`). That overlay is pure
//! text: no box, no fill, no `Clear`. btop with its menu open still shows
//! every panel, readable, around and between the glyphs.
//!
//! Two earlier versions got this wrong in opposite directions and both
//! were reported as "the menu hides everything": clearing the whole frame
//! before drawing the menu blanked the app, and filling a box of the theme
//! background punched a hole through the panel borders. These tests pin
//! the third answer.
//!
//! Every assertion here compares against the same app with the menu
//! closed. A cell's own background is not evidence of anything -- it
//! inherits the panel underneath -- so the question can only be answered
//! by what changed.

use doris::config::Config;
use doris::sources::models::TorrentItem;
use doris::ui::view::App as UiApp;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::Terminal;

fn make_app() -> UiApp {
    let mut app = UiApp::new("http://127.0.0.1:8090".into(), None);
    // Content long enough to reach across the middle of the menu, so a
    // hole punched through it would show.
    app.results = (0..30)
        .map(|i| TorrentItem {
            title: format!("row{i:02} {}", "A".repeat(90)),
            ..Default::default()
        })
        .collect();
    app.update_filter();
    app.zones.apply_preset("1,3|4");
    app.show_menu = true;
    app
}

fn draw(app: &mut UiApp, cfg: &Config) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| app.render(frame, cfg)).unwrap();
    terminal.backend().buffer().clone()
}

/// The cells the menu changed, with what they were before.
fn menu_diff(app: &mut UiApp, cfg: &Config) -> Vec<(u16, u16, Cell, Cell)> {
    let with = draw(app, cfg);
    app.show_menu = false;
    let without = draw(app, cfg);
    (0..with.area.height)
        .flat_map(|y| {
            (0..with.area.width)
                .map(move |x| (x, y))
                .collect::<Vec<_>>()
        })
        .filter(|&(x, y)| with[(x, y)].symbol() != without[(x, y)].symbol())
        .map(|(x, y)| (x, y, with[(x, y)].clone(), without[(x, y)].clone()))
        .collect()
}

/// The app is still there: the menu covers glyphs, not panels.
///
/// This is the claim that was reported twice. Clearing the frame first
/// left every cell under the menu blank; filling a box of the theme
/// background made the panel borders stop at the box and resume after it.
#[test]
fn test_the_app_stays_visible_behind_the_menu() {
    let mut app = make_app();
    let cfg = Config::default();
    let with = draw(&mut app, &cfg);
    app.show_menu = false;
    let without = draw(&mut app, &cfg);

    // The border row across the middle of the screen is still there
    // outside the menu's glyphs.
    let border_y = 17;
    let untouched = (0..with.area.width)
        .filter(|&x| with[(x, border_y)].symbol() == without[(x, border_y)].symbol())
        .count();
    assert!(
        untouched > with.area.width as usize / 2,
        "the row across the middle should be mostly untouched: {untouched} of {} cells",
        with.area.width
    );

    // The menu drew something, so the comparison above is meaningful.
    assert!(
        !menu_diff(&mut make_app(), &cfg).is_empty(),
        "the menu drew nothing"
    );
}

/// Nothing the menu draws carries a background of its own, whichever way
/// "Theme background" is set.
///
/// Both earlier attempts were reported as blocks: the per-glyph `main_bg`
/// slab under the banner and around the items, and the filled box that
/// covered the panel borders. A cell whose background *changed* is the
/// only evidence either happened.
#[test]
fn test_the_menu_paints_no_background() {
    for theme_background in [false, true] {
        let mut app = make_app();
        let cfg = Config {
            theme_background,
            ..Config::default()
        };
        let painted: Vec<_> = menu_diff(&mut app, &cfg)
            .into_iter()
            .filter(|(_, _, after, before)| after.bg != before.bg)
            .map(|(x, y, _, _)| (x, y))
            .collect();
        assert!(
            painted.is_empty(),
            "theme_background={theme_background}: the menu painted a background at {painted:?}"
        );
    }
}

/// Nothing in the menu is drawn in `title`, and the banner is drawn in
/// the accent.
///
/// With `primary` left unset the accent falls back to `title`, which in
/// the built-in theme is near-white -- so the ASCII banner came out the
/// same colour as the default foreground on a theme that already had an
/// accent of its own.
#[test]
fn test_the_banner_is_drawn_in_the_accent() {
    let mut app = make_app();
    let accent = app.theme.primary_color();
    let title = app.theme.title.to_color();
    let menu_fg = app.theme.menu_fg.to_color();
    assert_ne!(
        accent, title,
        "the built-in theme still has no accent of its own"
    );

    let painted: Vec<Color> = menu_diff(&mut app, &Config::default())
        .into_iter()
        .map(|(_, _, after, _)| after.fg)
        .collect();
    assert!(!painted.is_empty(), "the menu drew no glyph at all");
    let stray: Vec<_> = painted
        .iter()
        .filter(|fg| **fg != accent && **fg != menu_fg)
        .collect();
    assert!(
        stray.is_empty(),
        "the menu drew glyphs in colours it has no business using: {stray:?}"
    );
    assert!(
        !painted.contains(&title),
        "the banner came out in `title` ({title:?}) -- the accent fell back to it"
    );
    assert!(
        painted.contains(&accent),
        "no glyph carries the accent {accent:?}, so the banner is not in the theme's colour"
    );
}

/// The menu's own rect is where a click can be routed to an item, and it
/// refuses a terminal too small to hold it.
#[test]
fn test_the_menu_fits_a_terminal_that_can_hold_it() {
    let full = doris::ui::menu::menu_box_rect(Rect::new(0, 0, 100, 30)).expect("100x30 fits");
    assert!(
        doris::ui::menu::menu_box_rect(Rect::new(0, 0, full.width - 1, full.height)).is_none(),
        "one column short is refused"
    );
    assert!(
        doris::ui::menu::menu_box_rect(Rect::new(0, 0, full.width, full.height - 1)).is_none(),
        "one row short is refused"
    );
    assert!(
        doris::ui::menu::menu_box_rect(Rect::new(0, 0, full.width, full.height)).is_some(),
        "exactly fitting is drawn"
    );
    assert!(doris::ui::menu::menu_box_rect(Rect::new(0, 0, 10, 6)).is_none());
}
