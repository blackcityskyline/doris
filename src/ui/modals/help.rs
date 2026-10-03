//! The help page: a modal list of `[key, description]` pairs. It draws it
//! as a centred box titled `help` with the ASCII banner above it, a `Key:`/`Description:`
//! header line, one `[key, description]` pair per row from `HELP_TEXT` --
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

/// Columns the key column is padded to.
///
/// Padded, not truncated: a key wider than this would run into the
/// description column, which is why no key here is longer than 20
/// characters -- `ctrl + shift + arrows` became `ctrl+shift+arrows` for
/// exactly that reason.
const KEY_WIDTH: usize = 20;

/// Movement, zones, the three full-frame views, and getting out.
///
/// The pages are split by what a key is *for*, not by how many rows fit:
/// a table of thirty-four rows is a list to scroll, and a list that scrolls
/// is the heap this replaces. Anything about searching is on `2:search`,
/// anything about a torrent is on `4:torrents`.
pub const NAV_KEYS: &[(&str, &str)] = &[
    ("1, 2, 3, 4", "Focuses that zone; again hides it."),
    ("Tab, Shift+Tab", "Cycles focus between the visible zones."),
    ("ctrl + arrows", "Moves focus to the panel that way."),
    (
        "shift + arrows",
        "Swaps the focused panel with its neighbour.",
    ),
    ("ctrl+shift+arrows", "Resizes the focused panel that way."),
    ("Shift+P", "Cycles the saved zone layout (a preset)."),
    ("F", "Toggles fullscreen for the focused zone."),
    (
        "j, k, Up, Down",
        "Moves in the focused zone (j/k: Vim keys).",
    ),
    (
        "PageUp, PageDown",
        "Pages the results, the downloads or the log.",
    ),
    ("L", "Full-frame log."),
    ("T", "Full-frame downloads: cursor, controls, files."),
    ("R", "Full-frame results, with a preview line."),
    (
        "any of L, T, R",
        "From one, jumps to another; same key closes.",
    ),
    ("Esc", "Closes a modal or a mode; else opens the menu."),
    ("m", "The menu, from anywhere. Options lives in it."),
    ("q, ctrl + c", "Quits."),
    ("? , /, F1", "This window."),
    ("←, →", "Switches help page. So do 1-4."),
    (
        "Mouse click",
        "Focuses a zone; a frame button does its job.",
    ),
    ("Mouse scroll", "Scrolls what is under the cursor."),
    ("Mouse move", "Marks the frame button under the pointer."),
    ("Mouse drag", "Pulls the border between two panels."),
];

/// Searching and what to do with a row that came back.
pub const SEARCH_KEYS: &[(&str, &str)] = &[
    ("s, i, S", "Enters search input mode."),
    ("ctrl + u, ctrl + w", "Clears the input / deletes a word."),
    ("Enter", "Searches, or plays the selected row."),
    ("Esc", "Leaves input mode, keeping the query."),
    ("b", "Browse: freshest rows from every source."),
    ("g, G", "Cycles the category forward / back, and"),
    ("", "re-asks the checked sources for it."),
    ("Enter", "In Trackers, switches the row under it."),
    ("v", "Logs the selected row's details to the Log zone."),
    ("d", "Downloads the selected row's .torrent to disk."),
    ("Shift+Enter, D", "Row details: magnet, page, file list."),
];

/// The filter box: what it accepts, and where a row's category comes from
/// -- written down because those two are the parts of the UI a key list
/// cannot reach.
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

/// The Torrents panel and its full-frame view.
///
/// One page, because these keys only mean anything next to each other: a
/// key that acts on "the row under the cursor" is meaningless written down
/// without the cursor being on screen.
pub const TORRENT_KEYS: &[(&str, &str)] = &[
    ("2", "The Torrents panel: every download, live."),
    ("p", "Pauses / resumes the row under the cursor."),
    ("d", "Removes it and what it downloaded."),
    ("d, again", "Confirms the removal; any other key cancels."),
    ("T", "The full frame: same list, nothing dropped."),
    ("j, k, Up, Down", "Moves the cursor; the marker follows."),
    ("PgUp/PgDn/Home/End", "Moves it a page, to either end."),
    ("L, R", "From there, jumps to the log or results."),
    ("Esc", "Opens the menu, leaving the view; T closes it."),
    ("---", "In the full frame ---"),
    ("p", "Pause / resume the row under the cursor."),
    ("d", "Remove it and its data (confirm)."),
    ("v", "Verify the downloaded files against the hashes."),
    ("f", "The file list: what is fetched, per file."),
    ("o", "Opens the folder in yazi, tfm, elio, lf,"),
    ("", "ranger, nnn, vis, or a desktop file"),
    ("", "manager, or whatever xdg-open knows."),
    ("+", "Faster: walks up a fixed ramp of limits."),
    ("-", "Slower, and down off unlimited."),
    ("0", "Unlimited. It is the top rung, not a mode."),
    ("---", "In the file list (f) ---"),
    ("Enter", "Fetches that file / stops fetching it."),
    ("a", "All of them on."),
    ("n", "All of them off."),
    ("Esc", "Closes it."),
    ("---", "The same, without a terminal ---"),
    ("doris downloads", "The list, as a script wants it."),
    ("downloads --add", "Adds a magnet or a .torrent file."),
    ("doris torrent", "TorrServer, which is a different thing."),
];

pub fn sections() -> &'static [(&'static str, &'static [(&'static str, &'static str)])] {
    &[
        ("keys", NAV_KEYS),
        ("search", SEARCH_KEYS),
        ("filter", FILTER_HELP),
        ("torrents", TORRENT_KEYS),
    ]
}

/// A tab-stop'd key: the key is centred in a
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

        // Anything not a page key is consumed regardless: this
        let Modal::Help(state) = &mut self.modal else {
            return;
        };

        // Digits pick a section outright, the way the Options modal's
        // tabs do. Arrows walk; a digit says "this one", which is what
        // a user who already knows there are two tables wants.
        if let KeyCode::Char(c @ '1'..='9') = key.code {
            let idx = (c as u8 - b'1') as usize;
            {
                if idx < sections().len() {
                    let Modal::Help(state) = &mut self.modal else {
                        return;
                    };
                    if state.section != idx {
                        state.section = idx;
                        state.page = 0;
                    }
                    return;
                }
            }
        }

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
        // The help box is a fixed 78 columns wide -- nearly the
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
        // The first inner row is the `Key:` / `Description:` header, so
        // it is not available to the entries. Counting it as one made
        // the page count one too low whenever the table ended exactly
        // on the fold, and the last key was then dropped with no
        // second page and nothing to say so.
        let visible = (inner.height as usize).saturating_sub(1).max(1);
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
            .fg(self.theme.hi_fg.to_color())
            .add_modifier(Modifier::BOLD);
        let mut tabs: Vec<Span> = vec![Span::styled(" ◀ ", arrow)];
        for (n, (name, _)) in sections().iter().enumerate() {
            if n == section {
                tabs.push(Span::styled(format!("[{name}] "), active));
            } else {
                // The tab names the digit that opens it, exactly as the
                // Options tabs do: `2:network`, with the digit in the
                // keybind accent. A tab label that hides its own key
                // makes the digit binding below a secret.
                tabs.push(Span::styled(format!(" {}:", n + 1), arrow));
                tabs.push(Span::styled(format!("{name} "), inactive));
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

        // Structure in `primary`, the keybind column in `hi_fg`:
        let header_style = Style::default()
            .fg(self.theme.primary_color())
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
        for (key, desc) in table.iter().skip(start).take(visible) {
            rows.push(Line::from(vec![
                Span::styled(cjust(key, KEY_WIDTH), key_style),
                Span::styled(*desc, desc_style),
            ]));
        }

        frame.render_widget(Paragraph::new(rows), inner);
    }
}
