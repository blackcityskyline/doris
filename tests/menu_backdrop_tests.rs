//! The main menu must sit on something solid.
//!
//! `render_menu` draws its glyphs straight over the live zones, so the
//! rows *between* the banner and the items -- which carry no glyphs of
//! their own and so no highlight -- showed the zones' own content: the
//! menu looked like a sticker with the table showing through its gaps.
//! The menu now takes the same box a modal takes.

use doris::config::Config;
use doris::sources::models::TorrentItem;
use doris::ui::app::App as UiApp;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

/// The banner is six rows tall (`menu.rs::BANNER`); the row under it is
/// the spacing row, the one that used to be pure see-through.
const BANNER_ROWS: u16 = 6;

fn make_app() -> UiApp {
    let mut app = UiApp::new("http://127.0.0.1:8090".into(), None);
    // Titles long enough to reach across the box no matter where it
    // lands: what bleeds through has to be unmistakably zone content.
    app.results = (0..30)
        .map(|i| TorrentItem {
            title: format!("row{i:02} {}", "A".repeat(90)),
            ..Default::default()
        })
        .collect();
    app.update_filter();
    // Results alone, full height: with the default four-zone grid the
    // table stops halfway down the screen and the menu box lands on the
    // zones that have nothing to say -- the test would then prove that
    // empty space does not bleed through. One zone, and every row of the
    // box has table behind it.
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

#[test]
fn test_the_menu_box_covers_the_zone_content() {
    let mut app = make_app();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let buf = terminal.backend().buffer();

    let rect = doris::ui::menu::menu_backdrop_rect(Rect::new(0, 0, 120, 40))
        .expect("the box is drawn at this size");

    // `A` marks the seeded titles and appears nowhere in the banner, in
    // the items or in a frame border, so a stray one inside the box is
    // zone content seen through the menu -- while the same rows outside
    // the box are exactly where those titles keep being drawn.
    let rows = (rect.y + 1)..(rect.y + rect.height - 1);
    let cols = (rect.x + 1)..(rect.x + rect.width - 1);
    let mut inside = 0;
    let mut outside = 0;
    for y in rows {
        for x in 0..buf.area.width {
            if buf[(x, y)].symbol() != "A" {
                continue;
            }
            if cols.contains(&x) {
                inside += 1;
            } else {
                outside += 1;
            }
        }
    }

    println!("inside={inside} outside={outside} rect={rect:?}");
    assert_eq!(inside, 0, "zone content shows through the menu box");
    assert!(
        outside > 0,
        "the marker is real: the titles are still drawn beside the box"
    );
}

/// The row under the banner carries no glyph of its own, which is the
/// row that used to be a window: inside the box it is an empty row in
/// the modal's own background.
#[test]
fn test_the_gap_under_the_banner_is_the_modal_background() {
    let mut app = make_app();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let buf = terminal.backend().buffer();

    let rect = doris::ui::menu::menu_backdrop_rect(Rect::new(0, 0, 120, 40))
        .expect("the box is drawn at this size");
    let main_bg = app.theme.main_bg.to_color();

    let gap_y = rect.y + 1 + BANNER_ROWS;
    assert!(
        gap_y < rect.y + rect.height - 1,
        "the gap row is inside the box"
    );
    for x in (rect.x + 1)..(rect.x + rect.width - 1) {
        let cell = &buf[(x, gap_y)];
        assert_eq!(
            cell.symbol(),
            " ",
            "zone content bleeds into the menu gap at ({x},{gap_y})"
        );
        assert_eq!(
            cell.bg, main_bg,
            "the gap is the modal's background, not the terminal's, at ({x},{gap_y})"
        );
    }
}

#[test]
fn test_the_menu_box_carries_a_border_like_a_modal() {
    let mut app = make_app();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let buf = terminal.backend().buffer();

    let rect = doris::ui::menu::menu_backdrop_rect(Rect::new(0, 0, 120, 40))
        .expect("the box is drawn at this size");

    // A title in the top border, the way every other popup names itself.
    let title_row: String = (rect.x..rect.x + rect.width)
        .map(|x| buf[(x, rect.y)].symbol().chars().next().unwrap_or(' '))
        .collect();
    assert!(
        title_row.contains("menu"),
        "the top border is titled, got {title_row:?}"
    );
}
