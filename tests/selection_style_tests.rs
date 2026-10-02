//! Selection across the app draws in the theme's own selection colours
//! (`selected_bg`/`selected_fg`, `menu_selected_bg`/`_fg`). Every
//! bundled theme ships a value for them; the renderer never read any of
//! them, so the cursor was reverse video of whatever colours happened
//! to be in play.

use doris::config::Config;
use doris::sources::models::{FileEntry, TorrentItem};
use doris::ui::theme::Theme;
use doris::ui::view::{App as UiApp, Modal, TorrentDetailState};
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::Terminal;

mod common;

fn make_app() -> UiApp {
    common::make_app()
}

fn theme() -> Theme {
    Theme::default()
}

fn selected_bg() -> Color {
    theme().selected_bg.to_color()
}

fn selected_fg() -> Color {
    theme().selected_fg.to_color()
}

fn buffer(app: &mut UiApp, w: u16, h: u16) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn line_text(buf: &ratatui::buffer::Buffer, y: u16) -> String {
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol().to_string())
        .collect()
}

/// The (x, y) of `needle`'s first cell in the buffer.
fn find(buf: &ratatui::buffer::Buffer, needle: &str) -> Option<(u16, u16)> {
    for y in 0..buf.area.height {
        // Cell index, not byte index: a box-drawing glyph is three
        let line = line_text(buf, y);
        if let Some(byte) = line.find(needle) {
            return Some((line[..byte].chars().count() as u16, y));
        }
    }
    None
}

fn results(n: usize) -> Vec<TorrentItem> {
    (0..n)
        .map(|i| TorrentItem {
            title: format!("Torrent {i}"),
            ..Default::default()
        })
        .collect()
}

#[test]
fn test_results_cursor_uses_the_theme_selection_colours() {
    let mut app = make_app();
    app.results = results(5);
    app.update_filter();
    app.selected = 2;

    let buf = buffer(&mut app, 120, 40);
    let (x, y) = find(&buf, "Torrent 2").expect("the selected row is on screen");
    assert_eq!(
        buf[(x, y)].bg,
        selected_bg(),
        "the row is painted in selected_bg"
    );
    assert_eq!(
        buf[(x, y)].fg,
        selected_fg(),
        "and written in selected_fg, not reverse video"
    );
}

#[test]
fn test_a_row_that_is_not_selected_is_left_alone() {
    let mut app = make_app();
    app.results = results(5);
    app.update_filter();
    app.selected = 2;

    let buf = buffer(&mut app, 120, 40);
    let (x, y) = find(&buf, "Torrent 3").expect("the neighbour is on screen");
    assert_ne!(
        buf[(x, y)].bg,
        selected_bg(),
        "only the cursor row is painted"
    );
}

#[test]
fn test_sources_cursor_uses_the_theme_selection_colours() {
    let mut app = make_app();
    app.sources_cursor = 2; // the third row: `all`, `rutracker`, `rutor`

    let buf = buffer(&mut app, 120, 40);
    let (x, y) = find(&buf, "rutor").expect("the row is on screen");
    assert_eq!(buf[(x, y)].bg, selected_bg(), "the cursor row is painted");
    assert_eq!(buf[(x, y)].fg, selected_fg(), "and written in selected_fg");
}

/// The menu marks the picked item by its glyph colour, not by painting
/// it.
///
/// `menu_selected_bg` is the theme accent on several themes, so filling
/// the glyphs with it drew a bright block around the word under the
/// cursor. The picked item takes the accent as its foreground, bold, and
/// the menu paints no background of its own anywhere.
///
/// "Paints nothing" is checked against the same app with the menu closed
/// rather than against `Color::Reset`: the menu is drawn over the live
/// panels, so its cells carry whatever background the panel underneath
/// painted. Reading the cell's own background would be reading the panel.
#[test]
fn test_menu_selection_is_the_accent_colour_and_no_fill() {
    let mut app = make_app();
    app.show_menu = true;
    app.menu.selected = 1;

    let buf = buffer(&mut app, 120, 40);
    // The items are ASCII art, not words, and the picked one is drawn
    // from the doubled-line table -- so `Help` is looked for by its bold
    // glyphs, which are what distinguishes it from the two around it.
    let (x, y) = find(&buf, "╔═╴").expect("the picked menu item is on screen");
    assert_eq!(
        buf[(x, y)].fg,
        theme().primary_color(),
        "the picked item is written in the accent"
    );

    app.show_menu = false;
    let plain = buffer(&mut app, 120, 40);
    assert_eq!(
        buf[(x, y)].bg,
        plain[(x, y)].bg,
        "and the menu painted nothing behind it"
    );

    // The cell two to the left is a space inside the same line of art,
    // and the row below is an unpicked item.
    assert_eq!(buf[(x - 2, y)].symbol(), " ", "a space, not a glyph");
    assert_eq!(
        buf[(x - 2, y)].fg,
        theme().menu_fg.to_color(),
        "the unpicked colour"
    );
}

#[test]
fn test_detail_file_cursor_uses_the_theme_selection_colours() {
    let mut app = make_app();
    app.modal = Modal::TorrentDetail(Box::new(TorrentDetailState::new(TorrentItem {
        title: "Release".into(),
        ..Default::default()
    })));
    if let Modal::TorrentDetail(ref mut state) = app.modal {
        state.files = vec![
            FileEntry {
                name: "file_00.mkv".into(),
                size: "1 GB".into(),
            },
            FileEntry {
                name: "file_01.mkv".into(),
                size: "2 GB".into(),
            },
        ];
        state.pending = false;
        state.cursor = 1;
    }

    let buf = buffer(&mut app, 90, 30);
    let (x, y) = find(&buf, "file_01.mkv").expect("the file row is on screen");
    assert_eq!(buf[(x, y)].bg, selected_bg(), "the cursor row is painted");
    assert_eq!(buf[(x, y)].fg, selected_fg(), "and written in selected_fg");
}
