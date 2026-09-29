//! Colour distribution on the zones -- one rule for every theme: the
//! frame and the words that structure it take `primary`, the keybind
//! glyphs take `on_hover`, accents land on a few columns instead of
//! repainting the row, and values stay in the body colour. The colours
//! come from the theme's tokens, so nothing here names a raw colour.

use doris::config::Config;
use doris::sources::models::TorrentItem;
use doris::ui::app::App as UiApp;
use doris::ui::app::AppState;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui::Terminal;

fn make_app() -> UiApp {
    UiApp::new("http://127.0.0.1:8090".into(), None)
}

fn buffer(app: &mut UiApp, w: u16, h: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    terminal.backend().buffer().clone()
}

/// Where `needle` starts. The tests read a colour off a cell, so they
/// name the cell by its text -- the way a reader finds it.
fn find(buf: &Buffer, needle: &str) -> Option<(u16, u16)> {
    let w = needle.chars().count() as u16;
    for y in 0..buf.area.height {
        for x in 0..buf.area.width.saturating_sub(w.saturating_sub(1)) {
            let mut got = String::new();
            for i in 0..w {
                got.push_str(buf[(x + i, y)].symbol());
            }
            if got == needle {
                return Some((x, y));
            }
        }
    }
    None
}

fn fg_at(buf: &Buffer, needle: &str) -> Color {
    let (x, y) = find(buf, needle).unwrap_or_else(|| panic!("'{needle}' is not on screen"));
    buf[(x, y)].fg
}

fn one_result() -> TorrentItem {
    TorrentItem {
        title: "Some Torrent".to_string(),
        size: "1 GB".to_string(),
        seeds: "1234".to_string(),
        date: "2020-05-06".to_string(),
        ..Default::default()
    }
}

/// The header is structure (`primary`), and only two data columns carry
/// an accent: the seed count in `secondary`, the date in the
/// informational mid-bright. Size and title keep the body colour, so
/// the row reads as data and not as a rainbow.
#[test]
fn test_results_accents_two_columns_and_leave_the_rest_alone() {
    let mut app = make_app();
    app.results = vec![one_result()];
    app.update_filter();
    app.selected = usize::MAX; // no cursor row, so no selection style

    let buf = buffer(&mut app, 120, 40);
    let theme = doris::ui::theme::Theme::dark();

    assert_eq!(fg_at(&buf, "Seeds"), theme.primary_color());
    assert_eq!(fg_at(&buf, "1234"), theme.secondary_color());
    // The Date column is 8 wide, so the value is drawn truncated --
    // the cell is what is asserted, not the whole date.
    assert_eq!(fg_at(&buf, "2020-05"), theme.graph_text.to_color());
    assert_eq!(
        fg_at(&buf, "Some Torrent"),
        Color::Reset,
        "the title is body text: no accent token on it"
    );
}

/// Labels name the values (`secondary`), the values themselves stay in
/// the body colour -- and no label is a colour the theme never chose.
#[test]
fn test_torrent_labels_are_accented_and_values_are_body_text() {
    let mut app = make_app();
    app.state = AppState::Streaming;
    app.torrent_status.hash = "abcdef0123456789".to_string();
    app.torrent_status.status = "Downloading".to_string();

    let buf = buffer(&mut app, 120, 40);
    let theme = doris::ui::theme::Theme::dark();

    assert_eq!(fg_at(&buf, "Progress:"), theme.secondary_color());
    assert_eq!(fg_at(&buf, "DL:"), theme.secondary_color());
    assert_eq!(fg_at(&buf, "abcdef0123456789"), theme.main_fg.to_color());
}

/// The help page is the frame rule applied to a table: the header row
/// is structure (`primary`) and the key column -- the actionable half
/// of every row -- is `on_hover`.
#[test]
fn test_help_page_accents_the_header_and_the_keybind_column() {
    let mut app = make_app();
    app.open_help_modal();

    let buf = buffer(&mut app, 120, 40);
    let theme = doris::ui::theme::Theme::dark();

    assert_eq!(fg_at(&buf, "Key:"), theme.primary_color());
    assert_eq!(fg_at(&buf, "s, i"), theme.on_hover_color());
}
