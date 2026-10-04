//! One line of typing and a magnet in it.
//!
//! The Torrents panel is a list of what the daemon already holds, and `d`
//! on a search row puts a row there. Neither can start a torrent the user
//! has in no list at all: a magnet link from a message board, a `.torrent`
//! file somebody sent them. So there is a field for it, and a button on the
//! frame that opens it -- the same place every other doris action is
//! written down.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::config::Config;
use crate::ui::view::{App, Modal};

/// What the field holds, and what the daemon said about it last time.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MagnetState {
    pub input: String,
    /// In characters, not bytes: a magnet is ASCII but a pasted path may not
    /// be, and the field measures its text the way the search box does.
    pub cursor: usize,
    /// Shown in the dialog itself. A refusal in the log has scrolled away by
    /// the time the user looks for it, and "nothing happened" is the answer
    /// they cannot act on.
    pub error: Option<String>,
}

impl MagnetState {
    pub fn new() -> Self {
        Self::default()
    }

    fn chars(&self) -> usize {
        self.input.chars().count()
    }

    /// The caret's column, clamped: a cursor left behind by a shorter string
    /// would put the caret past the end of the text.
    fn at(&self) -> usize {
        self.cursor.min(self.chars())
    }

    fn byte_at_cursor(&self) -> usize {
        self.input
            .char_indices()
            .nth(self.at())
            .map(|(i, _)| i)
            .unwrap_or(self.input.len())
    }

    pub fn type_char(&mut self, c: char) {
        self.input.insert(self.byte_at_cursor(), c);
        self.cursor = self.at() + 1;
    }

    pub fn backspace(&mut self) {
        let at = self.at();
        if at == 0 {
            return;
        }
        if let Some((i, _)) = self.input.char_indices().nth(at - 1) {
            self.input.remove(i);
        }
        self.cursor = at - 1;
    }

    pub fn delete_under_cursor(&mut self) {
        let at = self.at();
        if at < self.chars() {
            self.input.remove(self.byte_at_cursor());
        }
    }

    /// Clamps at both ends, like the search box's caret: an arrow that
    /// teleports from the start of a magnet to its end changes what the next
    /// character does, and the only way to find that out is to type one.
    pub fn move_cursor(&mut self, delta: i64) {
        let next = (self.at() as i64 + delta).clamp(0, self.chars() as i64);
        self.cursor = next as usize;
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.chars();
    }

    pub fn cursor_column(&self) -> usize {
        self.at()
    }

    /// Whether what was typed is a magnet at all.
    ///
    /// Checked before the daemon is asked: `torrent-add` with a local path
    /// works, and this field is for links. A path that ends in `.torrent` is
    /// accepted too, because that is what a file manager drops into a
    /// terminal and the user pasting one means it.
    pub fn looks_addable(&self) -> bool {
        let text = self.input.trim();
        text.starts_with("magnet:") || text.to_lowercase().ends_with(".torrent")
    }
}

impl App {
    pub fn open_magnet_modal(&mut self) {
        self.modal = Modal::Magnet(Box::new(MagnetState::new()));
    }

    pub fn close_magnet_modal(&mut self) {
        if matches!(self.modal, Modal::Magnet(_)) {
            self.modal = Modal::None;
        }
    }

    /// What the field says, when it says something the daemon can take.
    ///
    /// Returning the string and letting the orchestrator do the asking is
    /// what every other modal here does: the daemon is not reachable from
    /// the view, and a dialog that cannot start anything is a text field.
    pub fn magnet_key(&mut self, key: KeyEvent) -> Option<String> {
        let Modal::Magnet(state) = &mut self.modal else {
            return None;
        };
        match key.code {
            KeyCode::Esc => self.close_magnet_modal(),
            KeyCode::Enter => {
                let text = state.input.trim().to_string();
                if !state.looks_addable() {
                    state.error = Some(if text.is_empty() {
                        "Nothing to add.".to_string()
                    } else {
                        format!("Not a magnet link and not a .torrent file: {text}")
                    });
                    return None;
                }
                state.error = None;
                return Some(text);
            }
            KeyCode::Left => state.move_cursor(-1),
            KeyCode::Right => state.move_cursor(1),
            KeyCode::Home => state.home(),
            KeyCode::End => state.end(),
            KeyCode::Delete => state.delete_under_cursor(),
            KeyCode::Backspace => state.backspace(),
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                state.type_char(c);
            }
            _ => {}
        }
        None
    }

    /// What the daemon answered, shown in the dialog that asked.
    pub fn magnet_says(&mut self, message: String) {
        if let Modal::Magnet(state) = &mut self.modal {
            state.error = Some(message);
        }
    }

    /// The dialog closes when the answer arrives: the answer is in the log,
    /// the field is empty, and a dialog left open over an empty field reads
    /// as a magnet that was not added.
    pub fn magnet_done(&mut self) {
        self.close_magnet_modal();
    }

    pub(crate) fn render_magnet_modal(&self, frame: &mut Frame, area: Rect, config: &Config) {
        let Modal::Magnet(state) = &self.modal else {
            return;
        };
        let theme = &self.theme;
        let label = Style::default().fg(theme.secondary_color());
        let body = Style::default().fg(theme.main_fg.to_color());
        let dim = Style::default().fg(theme.div_line.to_color());
        let hot = Style::default().fg(theme.hi_fg.to_color());

        let width = area.width.saturating_sub(8).clamp(40, 110);
        let height = 7;
        if area.height < height + 2 || area.width < width {
            return;
        }
        let popup = Rect {
            x: area.x + (area.width - width) / 2,
            y: area.y + (area.height - height) / 2,
            width,
            height,
        };
        let block = self
            .modal_block(theme.primary_color(), config)
            .title(Span::styled(
                " Add a magnet ",
                Style::default().fg(theme.primary_color()),
            ));
        let inner = block.inner(popup);
        frame.render_widget(Clear, popup);
        frame.render_widget(block, popup);

        // The field is the second row: a title, then the line being typed,
        // then the hint or the daemon's answer.
        // What is typed, and only a placeholder while there is nothing: a
        // `magnet: ` prefix in front of the field would end up in front of
        // every magnet too, since a magnet starts with those six letters.
        let caret = state.cursor_column();
        let (shown, style) = if state.input.is_empty() {
            (
                "magnet:?xt=urn:btih:…  or a path to a .torrent".to_string(),
                dim,
            )
        } else {
            (
                crate::ui::torrents_panel::truncate(&state.input, inner.width as usize),
                body,
            )
        };
        let _ = label;
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(shown, style))),
            Rect {
                y: inner.y,
                height: 1,
                ..inner
            },
        );
        let text_row = inner.y + 2;
        if let Some(error) = &state.error {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    crate::ui::torrents_panel::truncate(error, inner.width as usize),
                    Style::default().fg(theme.error_color()),
                )),
                Rect {
                    y: text_row,
                    height: 1,
                    ..inner
                },
            );
        } else {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("Enter", hot),
                    Span::styled(" adds it, ", dim),
                    Span::styled("Esc", hot),
                    Span::styled(" closes. A .torrent path works too.", dim),
                ])),
                Rect {
                    y: text_row,
                    height: 1,
                    ..inner
                },
            );
        }
        if caret < inner.width as usize {
            frame.set_cursor_position((inner.x + caret as u16, inner.y));
        }
    }
}
