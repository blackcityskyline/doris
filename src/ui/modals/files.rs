//! The file list of one download, and choosing what to fetch out of it.
//!
//! A torrent is not a file. Transmission can be told to fetch any subset of
//! one, per file, and the only way to reach that from here is to show the
//! list and let the cursor turn entries off -- which is what this modal is.
//! It is the reason a download can be a season and not a whole disk of it.

use ratatui::prelude::*;
use ratatui::widgets::*;

use crossterm::event::{KeyCode, KeyEvent};

use crate::config::Config;
use crate::ui::view::{App, Modal};

/// One torrent's files, and where the cursor is in them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilesState {
    pub id: i64,
    /// The torrent's name, for the title.
    pub name: String,
    pub files: Vec<crate::transmission::FileEntry>,
    pub cursor: usize,
    /// `true` while the daemon has not answered: the modal says so rather
    /// than showing an empty list, which reads as "this torrent is one
    /// empty file".
    pub pending: bool,
    /// Set when the list could not be read, so the modal can say why
    /// instead of looking like a torrent with no files.
    pub error: Option<String>,
}

impl FilesState {
    pub fn wanted_total(&self) -> i64 {
        self.files.iter().filter(|f| f.wanted).map(|f| f.size).sum()
    }

    pub fn all_total(&self) -> i64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

impl App {
    pub fn open_files_modal(&mut self, id: i64, name: String) {
        self.modal = Modal::Files(Box::new(FilesState {
            id,
            name,
            pending: true,
            ..FilesState::default()
        }));
    }

    /// The file list's own keys.
    pub fn files_key(&mut self, key: KeyEvent, vim_keys: bool) {
        let Modal::Files(state) = &mut self.modal else {
            return;
        };
        let len = state.files.len();
        match key.code {
            KeyCode::Esc => self.close_files_modal(),
            KeyCode::Down | KeyCode::Char('j') if vim_keys || matches!(key.code, KeyCode::Down) => {
                if len > 0 {
                    let next = (state.cursor + 1).min(len - 1);
                    state.cursor = next;
                }
            }
            KeyCode::Up | KeyCode::Char('k') if vim_keys || matches!(key.code, KeyCode::Up) => {
                state.cursor = state.cursor.saturating_sub(1);
            }
            KeyCode::PageDown => {
                if len > 0 {
                    state.cursor = (state.cursor + len / 2).min(len - 1);
                }
            }
            KeyCode::PageUp => state.cursor = state.cursor.saturating_sub(len / 2),
            KeyCode::Home => state.cursor = 0,
            KeyCode::End => state.cursor = len.saturating_sub(1),
            KeyCode::Enter => self.toggle_file_wanted(),
            KeyCode::Char('a') => self.set_all_files_wanted(true),
            KeyCode::Char('n') => self.set_all_files_wanted(false),
            _ => {}
        }
    }

    pub(super) fn close_files_modal(&mut self) {
        if matches!(self.modal, Modal::Files(_)) {
            self.modal = Modal::None;
        }
    }

    /// Turn the file under the cursor off, or back on.
    ///
    /// The local flag moves first so the key feels like it did something,
    /// and the change is queued in `pending_files` for the orchestrator to
    /// send: the daemon is not reachable from here, and a modal that cannot
    /// change anything is a list with no checkboxes.
    pub fn toggle_file_wanted(&mut self) {
        let Modal::Files(state) = &mut self.modal else {
            return;
        };
        if state.cursor >= state.files.len() {
            return;
        }
        let index = state.cursor;
        let wanted = !state.files[index].wanted;
        state.files[index].wanted = wanted;
        self.pending_files.push((index, wanted));
    }

    /// Every file at once, which is the other half of a per-file switch:
    /// the two buttons people reach for when a torrent turns out to hold
    /// something they did not want.
    pub fn set_all_files_wanted(&mut self, wanted: bool) {
        let Modal::Files(state) = &mut self.modal else {
            return;
        };
        for (index, file) in state.files.iter_mut().enumerate() {
            if file.wanted != wanted {
                file.wanted = wanted;
                self.pending_files.push((index, wanted));
            }
        }
    }

    pub(crate) fn render_files_modal(&self, frame: &mut Frame, area: Rect, config: &Config) {
        let Modal::Files(state) = &self.modal else {
            return;
        };
        let theme = &self.theme;
        let body = Style::default().fg(theme.main_fg.to_color());
        let label = Style::default().fg(theme.secondary_color());
        let dim = Style::default().fg(theme.div_line.to_color());

        let width = area.width.saturating_sub(8).clamp(30, 100);
        let height = area.height.saturating_sub(6).clamp(8, 24);
        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + (area.height.saturating_sub(height)) / 2,
            width,
            height,
        };

        let title = if state.pending {
            " Files ".to_string()
        } else {
            format!(
                " {} — {}/{} files, {} wanted ",
                crate::ui::torrents_panel::truncate(&state.name, popup.width as usize - 40),
                state.files.iter().filter(|f| f.wanted).count(),
                state.files.len(),
                crate::transmission::human_bytes(state.wanted_total().max(0) as u64),
            )
        };

        let mut lines: Vec<Line> = Vec::new();
        if let Some(error) = &state.error {
            lines.push(Line::from(Span::styled(
                error.clone(),
                Style::default()
                    .fg(theme.error_color())
                    .add_modifier(Modifier::BOLD),
            )));
        } else if state.pending {
            lines.push(Line::from(Span::styled("Asking the daemon…", body)));
        } else if state.files.is_empty() {
            lines.push(Line::from(Span::styled(
                "This torrent reports no files.",
                body,
            )));
        } else {
            let name_width = (popup.width as usize).saturating_sub(30).max(12);
            lines.push(Line::from(vec![
                Span::styled("    ", dim),
                Span::styled(format!("{:>name_width$}", "size"), label),
                Span::styled("  done  ", label),
                Span::styled("file", label),
            ]));
            let visible = (popup.height as usize).saturating_sub(6).max(1);
            let first = state
                .cursor
                .saturating_sub(visible.saturating_sub(1))
                .min(state.files.len().saturating_sub(1));
            for (offset, file) in state.files[first..].iter().take(visible).enumerate() {
                let idx = first + offset;
                let selected = idx == state.cursor;
                let mark = if file.wanted { "[x]" } else { "[ ]" };
                let style = if selected {
                    Style::default()
                        .fg(theme.hi_fg.to_color())
                        .add_modifier(Modifier::BOLD)
                } else if file.wanted {
                    body
                } else {
                    dim
                };
                let done = if file.size > 0 {
                    format!("{:>4.0}%", file.done * 100.0)
                } else {
                    "--".to_string()
                };
                lines.push(Line::from(vec![
                    Span::styled(if selected { "▸ " } else { "  " }, style),
                    Span::styled(mark, style),
                    Span::styled(
                        format!(
                            "{:>name_width$}",
                            crate::transmission::human_bytes(file.size.max(0) as u64)
                        ),
                        style,
                    ),
                    Span::styled(format!("  {done:>5}  "), style),
                    Span::styled(
                        crate::ui::torrents_panel::truncate(&file.name, name_width + 8),
                        style,
                    ),
                ]));
            }
            if state.files.len() > first + visible {
                lines.push(Line::from(Span::styled(
                    format!("… {} more", state.files.len() - first - visible),
                    dim,
                )));
            }
        }

        lines.push(Line::from(Span::styled(String::new(), body)));
        lines.push(self.files_legend());

        let block = self
            .themed_block(theme.primary_color(), config)
            .title(Span::styled(
                title,
                Style::default().fg(theme.primary_color()),
            ));
        frame.render_widget(Clear, popup);
        frame.render_widget(Paragraph::new(lines).block(block), popup);
    }

    /// The keys, in the same two styles the zone frames use.
    fn files_legend(&self) -> Line<'static> {
        let word = Style::default().fg(self.theme.title.to_color());
        let hot = Style::default()
            .fg(self.theme.hi_fg.to_color())
            .add_modifier(Modifier::BOLD);
        let mut spans = Vec::new();
        for (label, key) in [
            ("toggle", "⏎"),
            ("all", "a"),
            ("none", "n"),
            ("close", "Esc"),
        ] {
            if !spans.is_empty() {
                spans.push(Span::styled("  ", word));
            }
            spans.push(Span::styled(label.to_string(), word));
            spans.push(Span::styled(format!(" {key}"), hot));
        }
        Line::from(spans)
    }
}
