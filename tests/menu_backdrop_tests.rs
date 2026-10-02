//! The main menu is an overlay of glyphs, and nothing else.
//!
//! The reference builds the frame, prints it, and only then prints
//! an overlay string on top of it. That overlay is pure
//! text: no box, no fill, no `Clear`. The menu open still shows
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

/// A render of `app` at the size the other tests use.
fn buffer(app: &mut UiApp) -> Buffer {
    draw(app, &Config::default())
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

/// The picked menu item is drawn heavier, not just in another colour.
///
/// This is the reference's own focus mark: `menu_normal` draws the thin
/// strokes and `menu_selected` the doubled ones (`btop_menu.cpp:154`).
/// Shape rather than hue is the point -- on a theme whose accent sits
/// next to its plain foreground, colour alone left nothing to see, which
/// is how "which selector has focus?" went unanswerable on paper,
/// phoenix-night and solarized.
///
/// The art is the reference's, copied out rather than retyped: its QUIT
/// is a column wider in the bold table than in the thin one, so nothing
/// here retypes it and nothing centres an item on its own length. That
/// last part is the claim the length check below would get wrong.
#[test]
fn test_the_picked_menu_item_is_drawn_with_doubled_lines() {
    use doris::ui::menu::{MENU_ITEMS, MENU_ITEMS_BOLD, MENU_ITEM_WIDTHS};

    let doubled = |c: char| match c {
        '┌' => '╔',
        '─' => '═',
        '┐' => '╗',
        '│' => '║',
        '└' => '╚',
        '┘' => '╝',
        '├' => '╠',
        '┤' => '╣',
        '┬' => '╦',
        '┴' => '╩',
        '┼' => '╬',
        other => other,
    };

    for (idx, (thin, fat)) in MENU_ITEMS.iter().zip(MENU_ITEMS_BOLD).enumerate() {
        assert_eq!(
            thin.len(),
            fat.len(),
            "item {idx} changes its line count between the two tables"
        );
        for (row, (a, b)) in thin.iter().zip(fat.iter()).enumerate() {
            let a = a.trim_end();
            let b = b.trim_end();
            assert_eq!(
                a.chars().map(doubled).collect::<String>(),
                b,
                "item {idx} row {row} is not its own thin art doubled"
            );
        }
        // The width an item is drawn and clicked in is its own, not
        // whichever table happens to be longer this frame.
        assert!(
            MENU_ITEM_WIDTHS[idx] as usize
                >= fat[0]
                    .trim_end()
                    .chars()
                    .count()
                    .max(thin[0].trim_end().chars().count()),
            "item {idx} is drawn wider than the space it gets"
        );
    }
}

/// Picking an item does not move it: the word stays in the same column,
/// only its weight changes.
#[test]
fn test_picking_a_menu_item_does_not_move_it() {
    let area = Rect::new(0, 0, 100, 30);
    let rects = doris::ui::menu::menu_item_rects(area);
    assert_eq!(rects.len(), 3);
    // The widths are the reference's own (`menu_width`,
    // `btop_menu.cpp:171`), not each table's own length. The bold QUIT
    // is a column wider than the thin one, so a length taken from the art
    // moves the word the moment it is picked -- and every item would sit
    // at its own column rather than on the centre.
    let widths: Vec<u16> = rects.iter().map(|r| r.width).collect();
    assert_eq!(
        widths,
        doris::ui::menu::MENU_ITEM_WIDTHS.to_vec(),
        "an item is drawn in the width its art happens to be, not the fixed one"
    );
    // And the fixed width is the wider of that item's two tables, so the
    // picked word is never clipped.
    for (idx, rect) in rects.iter().enumerate() {
        let widest = doris::ui::menu::MENU_ITEMS[idx]
            .iter()
            .chain(doris::ui::menu::MENU_ITEMS_BOLD[idx].iter())
            .map(|row| row.trim_end().chars().count() as u16)
            .max()
            .expect("an item has rows");
        assert!(
            rect.width >= widest,
            "item {idx} is drawn in {} columns but its bold art needs {widest}",
            rect.width
        );
    }
    // And they are centred on the frame, not on themselves.
    for r in &rects {
        assert_eq!(
            r.x as i32,
            (area.width as i32 - r.width as i32) / 2,
            "the item is not centred on its own width: {r:?}"
        );
    }
}

/// No half stroke survives in the heavy art.
///
/// The thin art uses `╶ ╷ ╵ ╴` -- half a line, which is how the letters
/// get their shadow. Doubled, a half stroke has to become the *whole*
/// doubled line it was half of, or the drawing stops halfway along every
/// stroke that ends in one. That was visible as letters whose middle
/// bars ran out before reaching the edge.
#[test]
fn test_the_heavy_art_has_no_half_stroke_left_in_it() {
    use doris::ui::menu::MENU_ITEMS_BOLD;

    for (idx, block) in MENU_ITEMS_BOLD.iter().enumerate() {
        for (row, line) in block.iter().enumerate() {
            let left: Vec<char> = line.chars().filter(|c| "╶╴╷╵".contains(*c)).collect();
            assert!(
                left.is_empty(),
                "item {idx} row {row} still draws a half stroke: {left:?} in {line:?}"
            );
        }
    }
}

/// The picked item really is the one that comes out heavier, whichever
/// of the three it is.
///
/// Located by the art itself rather than by a guessed row: the banner is
/// drawn in doubled strokes too, so a row number would not say which item
/// it landed on. The markers come from the tables, and what is asserted
/// is that the renderer picked the right one of the pair.
#[test]
fn test_exactly_one_menu_item_is_heavy() {
    use doris::ui::menu::{MENU_ITEMS, MENU_ITEMS_BOLD};

    for selected in 0..3usize {
        let mut app = make_app();
        app.menu.selected = selected;
        let buf = buffer(&mut app);
        let rows: Vec<String> = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect();

        for idx in 0..3usize {
            // The middle row: the longest of the three and the one with
            // the most strokes in it.
            let thin = MENU_ITEMS[idx][1];
            let fat = MENU_ITEMS_BOLD[idx][1];
            let row = rows
                .iter()
                .find(|r| r.contains(thin) || r.contains(fat))
                .unwrap_or_else(|| panic!("item {idx} is not on screen at all"));
            if idx == selected {
                assert!(
                    row.contains(fat),
                    "the picked item {idx} is drawn thin: {row}"
                );
            } else {
                assert!(
                    row.contains(thin),
                    "item {idx} is heavy while {selected} is the picked one: {row}"
                );
            }
        }
    }
}

/// A frame button is bracketed, which is what tells it apart from the
/// panel title it sits beside: both are `primary`, and before the
/// brackets there was nothing else to tell them apart by.
#[test]
fn test_a_frame_button_is_bracketed() {
    let mut app = make_app();
    let buf = buffer(&mut app);
    let line: String = (0..buf.area.width)
        .map(|x| buf[(x, 3)].symbol().to_string())
        .collect();
    assert!(
        line.contains('┌') && line.contains('┐'),
        "the frame row should carry a bracketed button: {line}"
    );
    assert!(
        line.contains("┌filter┐") && line.contains("┌group┐"),
        "each button is bracketed: {line}"
    );
}

/// With "Show boxes" off there is no frame to bracket against, so the
/// brackets go with it -- otherwise the buttons would be the only thing
/// left drawing a box-drawing character.
#[test]
fn test_the_brackets_follow_show_boxes() {
    let mut app = make_app();
    let cfg = Config {
        show_boxes: false,
        ..Config::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| app.render(frame, &cfg)).unwrap();
    let buf = terminal.backend().buffer();
    // Only the panel's own border row, not the whole screen: the menu's
    // ASCII art is drawn in single strokes and has nothing to do with
    // "Show boxes".
    let border: String = (0..buf.area.width)
        .map(|x| buf[(x, 3)].symbol().to_string())
        .collect();
    assert!(
        !border.contains('┌') && !border.contains('┐'),
        "a button bracket survived with the frames turned off: {border}"
    );
}
