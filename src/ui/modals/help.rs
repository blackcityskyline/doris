//! The help page: doris' counterpart of btop's `helpMenu` (`btop_menu.cpp:1743`). btop draws it
//! as a centred box titled `help` with the ASCII banner above it, a `Key:`/`Description:`
//! header line, one `[key, description]` pair per row from `help_text` (`btop_menu.cpp:174`) --
//! the key in `hi_fg` + bold padded to 20 columns (`cjust(..., 20)`), the description in
//! `main_fg` -- and, when the table is taller than the box, an `↑ page 1/2 ↓` indicator on the
//! bottom border with `j`/`k`/`PageUp`/`Tab` flipping pages and `Esc`/`q`/`h`/`Space`/
//! `Enter`/`Backspace` closing it.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::config::Config;
use crate::ui::view::{centered_rect, App, Modal};

/// Columns the key column is padded to -- btop's `cjust(..., 20)`.
const KEY_WIDTH: usize = 20;

/// `[key, description]` pairs, in the order they are drawn: btop's `help_text`
/// (`btop_menu.cpp:174`).
pub const HELP_TEXT: &[(&str, &str)] = &[
    ("Mouse 1", "Clicks zones and frame buttons; the box types."),
    ("Mouse scroll", "Scrolls what is under the cursor."),
    ("Mouse move", "Marks the frame button under the pointer."),
    ("s, i, S", "Enters search input mode."),
    ("ctrl + u, ctrl + w", "Clears the input / deletes a word."),
    ("Enter", "Searches / plays; in Trackers, switches the row."),
    ("Shift+Enter, D", "Shows row details; D works everywhere."),
    ("b", "Browse: freshest rows from every source."),
    ("L", "Toggles the detailed log view."),
    ("T", "Toggles the torrent detail view."),
    ("R", "Toggles the results detail view."),
    ("f", "Filter mode; words, -not, src:, size:, seeds:."),
    ("F", "Toggles fullscreen for the focused zone."),
    ("m", "Toggles the main menu (Options lives there)."),
    ("1, 2, 3, 4", "Focuses that zone; again hides it."),
    ("Shift+P", "Cycles the saved zone layout (a preset)."),
    ("Tab, Shift+Tab", "Cycles focus between the visible zones."),
    (
        "j, k, Up, Down",
        "Moves in the focused zone (j/k: Vim keys).",
    ),
    (
        "PageUp, PageDown",
        "Pages the results or the log (Log focused).",
    ),
    ("g, G", "Cycles the category forward / back."),
    ("d", "Downloads a row / removes the torrent (Torrent)."),
    ("d, again", "Confirms the removal; any other key cancels."),
    ("v", "Logs the selected result's details."),
    ("p", "Pauses / resumes the tracked torrent."),
    ("Esc", "Closes a modal / leaves input; opens the menu."),
    ("q, ctrl + c", "Quits program."),
    ("←, →", "Switches help section (keys / filter)."),
    ("? , /, F1", "Shows this window."),
];

/// The second table: what the filter box accepts (`src/filter.rs`) and where a row's category
/// comes from -- written down because those two are the parts of the UI a key list cannot
/// reach.
pub const FILTER_HELP: &[(&str, &str)] = &[
    ("f", "Opens the filter box (Results focused)."),
    ("word", "Substring of title, size, source, group."),
    ("-word", "Negates: keeps the rows without it."),
    ("src:id", "That tracker only (tracker: is the alias)."),
    ("group:name", "That category (cat: is the alias)."),
    ("title:word", "Substring of the title alone."),
    ("size:>1gb", "Bytes; b/kb/mb/gb/tb."),
    ("seeds:>50", "Seed count; > < >= <= = all work."),
    ("Esc", "Clears the filter; cursor goes back."),
    ("all", "Category: every row, whatever it is tagged."),
    ("g, G", "Cycles category (or the frame arrows)."),
    ("tagged", "nnmclub, nyaa, tpb, yts, eztv, subsplease,"),
    ("", "torentino: the category is read off the row."),
    ("by query", "rutracker, rutor, x1337x are told the"),
    ("", "category, so an all-search cannot tag rows"),
    ("", "that were never asked for one."),
    ("no group", "An untagged row is listed in all only."),
];

pub fn sections() -> &'static [(&'static str, &'static [(&'static str, &'static str)])] {
    &[("keys", HELP_TEXT), ("filter & grouping", FILTER_HELP)]
}

/// A tab-stop'd key: btop's `cjust(text, 20)` centres the key in a
/// 20-column column, which is what makes the two columns line up.
fn cjust(text: &str, width: usize) -> String {
    format!("{:^width$}", text, width = width)
}

/// Which page of the help table is showing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HelpState {
    pub page: usize,
    /// Pages the table has at the current size, refreshed each render.
    pub pages: usize,
    pub visible: usize,
    /// Which table is showing: an index into [`sections`], 0 = keys.
    pub section: usize,
}

impl App {
    pub fn open_help_modal(&mut self) {
        self.modal = Modal::Help(HelpState::default());
    }

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
        let Modal::Help(state) = &mut self.modal else {
            return;
        };

        // The section switch sits above the guard below: a table that
        let step = match key.code {
            KeyCode::Right => Some(1usize),
            KeyCode::Left => Some(sections().len().saturating_sub(1)),
            _ => None,
        };
        if let Some(step) = step {
            state.section = (state.section + step) % sections().len();
            // The new table is measured from its own top: a page number
            state.page = 0;
            return;
        }

        if state.pages <= 1 {
            return;
        }

        let next = matches!(
            key.code,
            KeyCode::Down | KeyCode::Char('j') | KeyCode::PageDown | KeyCode::Tab
        );
        let prev = matches!(
            key.code,
            KeyCode::Up | KeyCode::Char('k') | KeyCode::PageUp | KeyCode::BackTab | KeyCode::Home
        );
        if next {
            state.page = (state.page + 1) % state.pages;
        } else if prev {
            state.page = (state.page + state.pages - 1) % state.pages;
        }
    }

    /// The help page itself: the box, the header, the visible slice of the section's table and
    /// the page indicator.
    pub fn render_help_modal(&mut self, frame: &mut Frame, area: Rect, config: &Config) {
        // btop's help box is a fixed 78 columns wide -- nearly the
        let popup = centered_rect(90, 85, area);
        frame.render_widget(Clear, popup);

        let section = match &self.modal {
            Modal::Help(state) => state.section,
            _ => 0,
        };
        let (section_name, table) = sections()[section.min(sections().len() - 1)];

        let mut block = self
            .modal_block(self.theme.primary_color(), config)
            .title(Span::styled(
                format!(" help: {section_name} "),
                Style::default().fg(self.theme.primary_color()),
            ));
        let inner = block.inner(popup);
        let visible = (inner.height as usize).max(1);
        let pages = table.len().div_ceil(visible);

        if let Modal::Help(state) = &mut self.modal {
            state.visible = visible;
            state.pages = pages;
            // The terminal shrank under an open modal: pull the page
            state.page = state.page.min(pages.saturating_sub(1));
        }

        // Both tables are named on the border, with the one being shown
        // marked. Without this the only sign that a second table exists
        // is the title text changing when you press Right, which is no
        // sign at all -- the page looked like one long list.
        let active = Style::default()
            .fg(self.theme.primary_color())
            .add_modifier(Modifier::BOLD);
        let inactive = Style::default().fg(self.theme.inactive_fg.to_color());
        let arrow = Style::default()
            .fg(self.theme.on_hover_color())
            .add_modifier(Modifier::BOLD);
        let mut tabs: Vec<Span> = vec![Span::styled(" ◀ ", arrow)];
        for (n, (name, _)) in sections().iter().enumerate() {
            if n == section {
                tabs.push(Span::styled(format!("[{name}] "), active));
            } else {
                tabs.push(Span::styled(format!(" {name} "), inactive));
            }
        }
        tabs.push(Span::styled(" ▶ ", arrow));

        if pages > 1 {
            let page = match &self.modal {
                Modal::Help(state) => state.page,
                _ => 0,
            };
            tabs.push(Span::styled(" ↑ ", arrow));
            tabs.push(Span::styled(
                format!("page {}/{} ", page + 1, pages),
                active,
            ));
            tabs.push(Span::styled("↓", arrow));
        }
        block = block.title_bottom(Line::from(tabs));
        frame.render_widget(block, popup);

        // Structure in `primary`, the keybind column in `on_hover`:
        let header_style = Style::default()
            .fg(self.theme.primary_color())
            .add_modifier(Modifier::BOLD);
        let key_style = Style::default()
            .fg(self.theme.on_hover_color())
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
        for (key, desc) in table.iter().skip(start).take(visible) {
            rows.push(Line::from(vec![
                Span::styled(cjust(key, KEY_WIDTH), key_style),
                Span::styled(*desc, desc_style),
            ]));
        }

        frame.render_widget(Paragraph::new(rows), inner);
    }
}
