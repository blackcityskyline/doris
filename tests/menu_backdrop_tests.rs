//! The main menu must sit on something solid. `render_menu` draws its glyphs straight over the
//! live zones, so the rows *between* the banner and the items -- which carry no glyphs of their
//! own and so no highlight -- showed the zones' own content: the menu looked like a sticker
//! with the table showing through its gaps.

use doris::config::Config;
use doris::sources::models::TorrentItem;
use doris::ui::view::App as UiApp;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

/// The banner is six rows tall (`menu.rs::BANNER`); the row under it is
/// the spacing row, the one that used to be pure see-through.
const BANNER_ROWS: u16 = 6;

fn make_app() -> UiApp {
    let mut app = UiApp::new("http://127.0.0.1:8090".into(), None);
    // Titles long enough to reach across the box no matter where it
    app.results = (0..30)
        .map(|i| TorrentItem {
            title: format!("row{i:02} {}", "A".repeat(90)),
            ..Default::default()
        })
        .collect();
    app.update_filter();
    // Results alone, full height: with the default four-zone grid the
    app.zones.apply_preset("1");
    app.show_menu = true;
    app
}

#[test]
fn test_the_menu_box_fits_banner_and_items() {
    let area = Rect::new(0, 0, 120, 40);
    let rect = doris::ui::menu::menu_backdrop_rect(area).expect("a 120x40 terminal fits the menu");
    assert!(
        rect.width > 30 && rect.height > 20,
        "the box wraps the banner and the three items: {rect:?}"
    );
    assert!(
        area.x < rect.x
            && rect.x + rect.width <= area.x + area.width
            && area.y < rect.y
            && rect.y + rect.height <= area.y + area.height,
        "and stays on screen: {rect:?}"
    );
}

/// The row under the banner carries no glyph of its own, which is the
/// row that used to be a window: inside the box it is an empty row in
/// the modal's own background.

#[test]
fn test_the_menu_has_no_border_of_its_own() {
    let mut app = make_app();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let buf = terminal.backend().buffer();

    let rect = doris::ui::menu::menu_backdrop_rect(Rect::new(0, 0, 120, 40))
        .expect("the backdrop is drawn at this size");

    // The menu is an overlay, not a zone: no frame glyphs, no title.
    // A bordered box read as a fifth panel the user could not click.
    for (label, y) in [("top", rect.y), ("bottom", rect.y + rect.height - 1)] {
        let row: String = (rect.x..rect.x + rect.width)
            .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect();
        for corner in ['╭', '╰', '─', '│'] {
            assert!(
                !row.contains(corner),
                "the {label} edge of the menu must not be framed, got {row:?}"
            );
        }
    }
    for x in [rect.x, rect.x + rect.width - 1] {
        let col: String = (rect.y..rect.y + rect.height)
            .map(|y| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(
            !col.contains('│'),
            "the side of the menu must not be framed, got {col:?}"
        );
    }
}

/// The pure geometry, no rendering: the box wraps the banner and the
/// three items on a terminal that can hold them, is centred on the
/// screen, and is *absent* one row or one column short of that --
/// `None` is what tells the renderer to draw the menu without a
/// backdrop, the way it did before there was one, rather than clipping
/// a frame around something bigger than itself.
#[test]
fn test_the_backdrop_rect_is_centred_and_refuses_a_terminal_that_cannot_hold_it() {
    let full = doris::ui::menu::menu_backdrop_rect(Rect::new(0, 0, 120, 40))
        .expect("a 120x40 terminal holds the menu");

    let left_margin = full.x;
    let right_margin = 120 - (full.x + full.width);
    assert!(
        left_margin.abs_diff(right_margin) <= 1,
        "centred (odd widths round down): left={left_margin} right={right_margin}"
    );
    let top_margin = full.y;
    let bottom_margin = 40 - (full.y + full.height);
    assert!(
        top_margin.abs_diff(bottom_margin) <= 1,
        "vertically centred too: top={top_margin} bottom={bottom_margin}"
    );

    assert!(
        doris::ui::menu::menu_backdrop_rect(Rect::new(0, 0, full.width - 1, full.height)).is_none(),
        "one column short of the widest line and there is no box"
    );
    assert!(
        doris::ui::menu::menu_backdrop_rect(Rect::new(0, 0, full.width, full.height - 1)).is_none(),
        "one row short of banner + items + border and there is no box"
    );
    assert!(
        doris::ui::menu::menu_backdrop_rect(Rect::new(0, 0, full.width, full.height)).is_some(),
        "and exactly that size is enough"
    );
    assert!(
        doris::ui::menu::menu_backdrop_rect(Rect::new(0, 0, 10, 6)).is_none(),
        "a menu is not drawn into 10x6"
    );
}

/// Zone content must never show through a *glyph* of the menu.
///
/// The backdrop hugs the glyphs rather than filling one big rectangle,
/// which is the reference's shape -- a filled panel around a transient
/// overlay read as a fifth zone. So the rows the menu actually writes
/// have to be opaque, or the table shows through the letters.
#[test]
fn test_no_zone_content_shows_through_a_menu_glyph_row() {
    let mut app = make_app();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let buf = terminal.backend().buffer();

    let rect = doris::ui::menu::menu_backdrop_rect(Rect::new(0, 0, 120, 40))
        .expect("the menu is drawn at this size");

    // `A` marks the seeded titles and appears nowhere in the banner or
    // in the items, so an `A` on one of these rows is a hole.
    let glyph_rows = (rect.y + 1)..(rect.y + 1 + BANNER_ROWS);
    let mut holes = 0;
    for y in glyph_rows {
        let x0 = rect.x;
        let x1 = (x0 + 24).min(buf.area.width);
        for x in x0..x1 {
            if buf[(x, y)].symbol() == "A" {
                holes += 1;
            }
        }
    }
    assert_eq!(holes, 0, "zone content shows through the menu's own glyphs");
}

/// The gap between the banner and the first item is transparent, and
/// that is deliberate: the reference lets the list behind show through
/// there too. Filling it is what made the menu read as a panel.
#[test]
fn test_the_gap_between_banner_and_items_is_not_filled() {
    let mut app = make_app();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let buf = terminal.backend().buffer();

    let rect = doris::ui::menu::menu_backdrop_rect(Rect::new(0, 0, 120, 40))
        .expect("the menu is drawn at this size");
    let gap_y = rect.y + 1 + BANNER_ROWS;
    // Only the banner's own width is cleared -- the menu hugs its glyphs,
    // it does not span the whole terminal.
    let banner_w = 29; // DORIS banner width
    let start_x = (buf.area.width - banner_w) / 2;
    let painted = (start_x..start_x + banner_w)
        .filter(|&x| buf[(x, gap_y)].symbol() != " ")
        .count();
    assert_eq!(
        painted, 0,
        "the gap row under the banner is cleared, so zone content does not show through"
    );
}

/// The cells the menu writes, found by rendering the same app twice --
/// once with the menu up, once with it down -- and diffing. That names
/// them exactly, instead of guessing which cells inside the menu's box
/// belong to it: with "Theme background" on, the zones *behind* paint
/// their own background over everything the menu did not touch, and a
/// test that swept the whole box was really measuring them.
fn menu_cells(mut app: UiApp, cfg: &Config) -> Vec<(i16, i16, ratatui::buffer::Cell)> {
    let mut render = |menu: bool| {
        app.show_menu = menu;
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| app.render(frame, cfg)).unwrap();
        terminal.backend().buffer().clone()
    };
    let with = render(true);
    let without = render(false);
    let mut out = Vec::new();
    for y in 0..with.area.height {
        for x in 0..with.area.width {
            let (a, b) = (with[(x, y)].clone(), without[(x, y)].clone());
            if a.symbol() != b.symbol() || a.fg != b.fg || a.bg != b.bg {
                out.push((x as i16, y as i16, a));
            }
        }
    }
    out
}

/// The menu paints no background, whichever way "Theme background" is set.
///
/// The menu is an overlay, so anything it paints behind its glyphs sits
/// *on top of* the live zones. Both ways of doing that were wrong: the
/// per-glyph `main_bg` slab showed under the banner and around the
/// items, and the picked item's own `menu_selected_bg` -- which is the
/// theme accent on several themes -- drew a bright block around the word
/// under the cursor. Selection is carried by the glyph colour instead.
#[test]
fn test_the_menu_paints_no_background() {
    use ratatui::style::Color;

    for theme_background in [false, true] {
        let cfg = Config {
            theme_background,
            ..Config::default()
        };
        let painted: Vec<_> = menu_cells(make_app(), &cfg)
            .into_iter()
            .filter(|(_, _, c)| c.bg != Color::Reset)
            .collect();
        assert!(
            painted.is_empty(),
            "theme_background={theme_background}: the menu painted a background \
             behind itself at {painted:?}"
        );
    }
}

/// The ASCII banner is drawn in one colour: the theme's accent.
///
/// With `primary` left unset the accent falls back to `title`, which is
/// near-white in the built-in theme -- so the one piece of ASCII art on
/// screen came out the same colour as the default foreground, on a theme
/// that already had an accent of its own.
#[test]
fn test_the_banner_is_drawn_in_the_accent() {
    let app = make_app();
    let accent = app.theme.primary_color();
    let title = app.theme.title.to_color();
    assert_ne!(
        accent, title,
        "the built-in theme still has no accent of its own"
    );

    let cfg = Config::default();
    let cells = menu_cells(app, &cfg);
    let top = cells
        .iter()
        .map(|(_, y, _)| *y)
        .min()
        .expect("the menu drew something");
    let fgs: Vec<_> = cells
        .iter()
        .filter(|(_, y, _)| *y < top + doris::ui::menu::BANNER_ROWS as i16)
        .map(|(_, _, c)| c.fg)
        .collect();
    assert!(
        !fgs.is_empty() && fgs.iter().all(|fg| *fg == accent),
        "the banner must be one colour, the accent {accent:?}; saw {fgs:?}"
    );
}
