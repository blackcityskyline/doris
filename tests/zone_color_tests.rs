//! Colour distribution on the zones -- one rule for every theme: the
//! frame and the words that structure it take `primary`, the keybind
//! glyphs take `on_hover`, accents land on a few columns instead of
//! repainting the row, and values stay in the body colour. The colours
//! come from the theme's tokens, so nothing here names a raw colour.

use doris::config::Config;
use doris::sources::models::TorrentItem;
use doris::ui::app::App as UiApp;
use doris::ui::app::AppState;
use doris::ui::app::Modal;
use doris::ui::app::TorrentDetailState;
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

/// Where the first cell painted with `color` sits, so a test can name
/// the colour a panel chose instead of hunting for text over it.
fn bg_at(buf: &Buffer, color: Color) -> Option<(u16, u16)> {
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            if buf[(x, y)].bg == color {
                return Some((x, y));
            }
        }
    }
    None
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

/// The paging row is the frame rule again: the arrows are the glyphs
/// that act, so they take `on_hover` + bold, and the `page n/m` they
/// move is structure in `primary` -- btop draws exactly that split
/// (`btop_menu.cpp:1655` and `:1780`), and the Settings modal already
/// follows it. The help page drew the whole line unstyled, so the
/// arrows read as ordinary text.
#[test]
fn test_help_paging_arrows_take_the_hotkey_accent() {
    let mut app = make_app();
    app.open_help_modal();

    // 80x24 is the size the page-count test uses: two pages, so the
    // indicator is drawn at all.
    let buf = buffer(&mut app, 80, 24);
    let theme = doris::ui::theme::Theme::dark();

    assert_eq!(fg_at(&buf, "↑"), theme.on_hover_color());
    assert_eq!(fg_at(&buf, "page 1/2"), theme.primary_color());
    assert_eq!(fg_at(&buf, "↓"), theme.on_hover_color());
}

/// The popup's content sits on the theme's own background. btop paints
/// no content background at all (`createBox` writes plain spaces, so the
/// box shows the terminal through), and our `modal_block` already
/// fills with `main_bg` when "Theme background" is on -- the hardcoded
/// `DarkGray` these two modals used overrode both, and on a light
/// theme it put dark body text on grey.
#[test]
fn test_modal_content_sits_on_the_theme_background() {
    let theme = doris::ui::theme::Theme::dark();

    let mut detail = make_app();
    detail.modal = Modal::TorrentDetail(Box::new(TorrentDetailState::new(one_result())));
    let buf = buffer(&mut detail, 120, 40);
    assert_eq!(
        bg_at(&buf, Color::DarkGray),
        None,
        "the detail modal paints a background no theme chose"
    );
    let (x, y) = find(&buf, "Title:").expect("the facts are drawn");
    assert_eq!(buf[(x, y)].bg, theme.main_bg.to_color());

    let mut health = make_app();
    health.modal = Modal::HealthCheck(vec!["=== sources ===".into(), "✔ every source ok".into()]);
    let buf = buffer(&mut health, 120, 40);
    assert_eq!(
        bg_at(&buf, Color::DarkGray),
        None,
        "the health modal paints a background no theme chose"
    );
    let (x, y) = find(&buf, "✔ every source ok").expect("the check lines are drawn");
    assert_eq!(buf[(x, y)].bg, theme.main_bg.to_color());
}

/// The cursor row in the Options list is a selected row like any other:
/// btop paints it `selected_bg` + `selected_fg` (`btop_menu.cpp:1687`),
/// and Results, Trackers and the detail modal's file list already use
/// `selection_style()`. This list only recoloured the label to the
/// highlight accent, so the same cursor looked different here.
#[test]
fn test_the_settings_cursor_row_uses_the_selection_colours() {
    let mut app = make_app();
    app.open_settings(&Config::default(), false);

    // The cursor's own label, read out of the state rather than
    // hardcoded: the list grows whenever an option is added.
    let label = match &app.modal {
        Modal::Settings(state) => {
            let cat = &state.categories[state.selected_category];
            format!("{} 1/{}", cat.items[0].label, cat.items.len())
        }
        other => panic!("expected the Settings modal, got {other:?}"),
    };

    let buf = buffer(&mut app, 120, 40);
    let theme = doris::ui::theme::Theme::dark();

    let (x, y) = find(&buf, &label).expect("the option under the cursor is drawn");
    assert_eq!(
        buf[(x, y)].bg,
        theme.selected_bg.to_color(),
        "the cursor row is painted with selected_bg"
    );
    assert_eq!(buf[(x, y)].fg, theme.selected_fg.to_color());
}

/// The tab row marks the key, not the word: the brackets and the digit
/// that switches to a tab take `on_hover`, the tab's own name stays
/// structure in `primary` -- btop's split exactly (`btop_menu.cpp:1631`)
/// and the same rule the frame legend follows. Every tab was drawn in
/// one colour, so the digit you press looked like part of the label.
#[test]
fn test_settings_tab_markers_carry_the_accent_and_the_names_do_not() {
    let mut app = make_app();
    app.open_settings(&Config::default(), false);

    let buf = buffer(&mut app, 120, 40);
    let theme = doris::ui::theme::Theme::dark();

    let (x, y) = find(&buf, "[general]").expect("the selected tab is drawn");
    assert_eq!(buf[(x, y)].fg, theme.on_hover_color(), "'[' marks the tab");
    assert_eq!(
        buf[(x + 1, y)].fg,
        theme.primary_color(),
        "the tab's name is structure, not the mark"
    );
    assert_eq!(
        buf[(x + 9, y)].fg,
        theme.on_hover_color(),
        "']' marks the tab too"
    );

    let (x, y) = find(&buf, "2:streaming").expect("the next tab is drawn");
    assert_eq!(
        buf[(x, y)].fg,
        theme.on_hover_color(),
        "'2' is the key that switches to that tab"
    );
    assert_eq!(buf[(x + 2, y)].fg, theme.primary_color());
}
