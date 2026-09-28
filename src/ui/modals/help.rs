//! The help page: doris' counterpart of btop's `helpMenu`
//! (`btop_menu.cpp:1743`).
//!
//! btop draws it as a centred box titled `help` with the ASCII banner
//! above it, a `Key:`/`Description:` header line, one `[key, description]`
//! pair per row from `help_text` (`btop_menu.cpp:174`) -- the key in
//! `hi_fg` + bold padded to 20 columns (`cjust(..., 20)`), the
//! description in `main_fg` -- and, when the table is taller than the
//! box, an `↑ page 1/2 ↓` indicator on the bottom border with
//! `j`/`k`/`PageUp`/`Tab` flipping pages and `Esc`/`q`/`h`/`Space`/
//! `Enter`/`Backspace` closing it.
//!
//! One structural difference: btop computes the page count inside the
//! draw function, where the terminal size is in scope, and reads it back
//! from `static` state when a key arrives. Rust's `&self`/`&mut self`
//! split makes that state explicit instead: the renderer writes
//! [`HelpState::pages`] and [`HelpState::visible`] on every pass, and
//! `help_key` clamps against the numbers the renderer last used.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::ui::app::{centered_rect, App, Modal};

/// Columns the key column is padded to -- btop's `cjust(..., 20)`.
const KEY_WIDTH: usize = 20;

/// `[key, description]` pairs, in the order they are drawn: btop's
/// `help_text` (`btop_menu.cpp:174`). Kept public so a test can check
/// it still names every keybind AGENTS.md documents.
pub const HELP_TEXT: &[(&str, &str)] = &[
    ("Mouse 1", "Clicks zones, frame buttons, tabs, the search box."),
    ("Mouse scroll", "Scrolls what is under the cursor."),
    ("s, i", "Enters search input mode."),
    ("Enter", "Searches / plays; in Sources, switches the row."),
    ("Shift+Enter", "Shows the selected row's details."),
    ("b", "Browse: freshest rows from every source."),
    ("S", "Shows Options (Settings)."),
    ("L", "Toggles the detailed log view."),
    ("F", "Filter mode; Enter applies, Esc clears."),
    ("f", "Toggles fullscreen for the focused zone."),
    ("m", "Toggles the main menu."),
    ("1, 2, 3, 4, 5", "Toggles that zone's visibility."),
    ("Shift+P", "Cycles the layout preset (horizontal / split)."),
    ("Tab, Shift+Tab", "Cycles focus between the visible zones."),
    ("j, k, Up, Down", "Moves in the focused zone (j/k: Vim keys)."),
    ("g, G", "Cycles the category row forward / back."),
    ("d", "Downloads a row / removes the torrent."),
    ("v", "Logs the selected result's details."),
    ("p", "Pauses / resumes the tracked torrent."),
    ("Esc", "Closes a modal; leaves input / filter mode."),
    ("q, ctrl + c", "Quits program."),
    ("? , /, F1", "Shows this window."),
];

/// A tab-stop'd key: btop's `cjust(text, 20)` centres the key in a
/// 20-column column, which is what makes the two columns line up.
fn cjust(text: &str, width: usize) -> String {
    format!("{:^width$}", text, width = width)
}

/// Which page of the help table is showing.
///
/// `pages` and `visible` are filled in by the renderer rather than
/// computed by the key handler: how many rows fit depends on the
/// terminal, and only the draw pass knows the area.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HelpState {
    /// 0-based page index.
    pub page: usize,
    /// Pages the table has at the current size, refreshed each render.
    pub pages: usize,
    /// Rows that fit on one page, refreshed each render.
    pub visible: usize,
}

impl App {
    /// Re-open the help page at the top. btop does the same: its
    /// `helpMenu` resets `page = 0` whenever `bg` is empty, and `bg` is
    /// cleared when the box closes.
    pub fn open_help_modal(&mut self) {
        self.modal = Modal::Help(HelpState::default());
    }

    /// One keypress while the help page is open. Every key is consumed
    /// -- btop's `helpMenu` returns `NoChange` for anything it doesn't
    /// recognise, which is how a modal swallows the keys behind it --
    /// so the caller does not need to check whether anything matched.
    pub fn help_key(&mut self, key: KeyEvent) {
        if matches!(
            key.code,
            KeyCode::Esc
                | KeyCode::Char('q')
                | KeyCode::Char('h')
                | KeyCode::Char(' ')
                | KeyCode::Enter
                | KeyCode::Backspace
        ) {
            self.modal = Modal::None;
            return;
        }

        // Anything not a page key is consumed regardless: btop's
        // `helpMenu` answers unrecognised keys with `NoChange`, which is
        // how a modal keeps the keys behind it from firing.
        let Modal::Help(state) = &mut self.modal else {
            return;
        };
        if state.pages <= 1 {
            return;
        }

        let next = matches!(
            key.code,
            KeyCode::Down | KeyCode::Char('j') | KeyCode::PageDown | KeyCode::Tab
        );
        let prev = matches!(
            key.code,
            KeyCode::Up
                | KeyCode::Char('k')
                | KeyCode::PageUp
                | KeyCode::BackTab
                | KeyCode::Home
        );
        if next {
            state.page = (state.page + 1) % state.pages;
        } else if prev {
            state.page = (state.page + state.pages - 1) % state.pages;
        }
    }

    /// The help page itself: the box, the header, the visible slice of
    /// [`HELP_TEXT`] and the page indicator.
    ///
    /// `&mut self` because it publishes `pages`/`visible` for
    /// [`App::help_key`] on the way through.
    pub fn render_help_modal(&mut self, frame: &mut Frame, area: Rect) {
        // btop's help box is a fixed 78 columns wide -- nearly the
        // whole terminal on an 80-column screen -- because a two-column
        // table has no room to spare. Same intent here: wide enough
        // that no description is cut off at 80 columns.
        let popup = centered_rect(90, 85, area);
        frame.render_widget(Clear, popup);

        let mut block = self.modal_block(Color::Cyan).title(" help ");
        let inner = block.inner(popup);
        let visible = (inner.height as usize).max(1);
        let pages = HELP_TEXT.len().div_ceil(visible);

        if let Modal::Help(state) = &mut self.modal {
            state.visible = visible;
            state.pages = pages;
            // The terminal shrank under an open modal: pull the page
            // back into range rather than drawing past the end.
            state.page = state.page.min(pages.saturating_sub(1));
        }

        if pages > 1 {
            let page = match &self.modal {
                Modal::Help(state) => state.page,
                _ => 0,
            };
            block = block.title_bottom(Line::from(format!(
                " ↑ page {}/{} ↓",
                page + 1,
                pages
            )));
        }
        frame.render_widget(block, popup);

        let header_style = Style::default()
            .fg(self.theme.title.to_color())
            .add_modifier(Modifier::BOLD);
        let key_style = Style::default()
            .fg(self.theme.hi_fg.to_color())
            .add_modifier(Modifier::BOLD);
        let desc_style = Style::default().fg(self.theme.main_fg.to_color());

        let mut rows = vec![Line::from(vec![
            Span::styled(cjust("Key:", KEY_WIDTH), header_style),
            Span::styled("Description:", header_style),
        ])];
        let start = match &self.modal {
            Modal::Help(state) => state.page * visible,
            _ => 0,
        };
        for (key, desc) in HELP_TEXT.iter().skip(start).take(visible) {
            rows.push(Line::from(vec![
                Span::styled(cjust(key, KEY_WIDTH), key_style),
                Span::styled(*desc, desc_style),
            ]));
        }

        frame.render_widget(Paragraph::new(rows), inner);
    }
}
