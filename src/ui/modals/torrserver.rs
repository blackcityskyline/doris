//! TorrServer's own torrent list, and what can be done with a row.
//!
//! The Download zone is the *download daemon's* list (Transmission), which
//! says nothing about what the streaming server holds. This panel is the
//! other machine state: the torrents TorrServer knows, with play, pause,
//! remove, the file list, and a save-to-disk that uses the same stream mpv
//! would have read.
//!
//! It is zone `5`, so it tiles like the other four; `Shift+5` draws the
//! same panel over the tiling.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::config::Config;
use crate::torrserver::api::{TorrentFile, TorrentInfo};
use crate::ui::layout::ZoneId;
use crate::ui::view::App;

/// Which list the cursor is in: the torrents, or the selected one's files.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TorrServerState {
    pub cursor: usize,
    pub file_cursor: usize,
    /// `v`: the file list of the selected torrent, open under it.
    pub files_open: bool,
}

/// What a key asks the orchestrator to do. The panel moves cursors itself;
/// anything that talks to TorrServer comes back as one of these.
#[derive(Clone, Debug, PartialEq)]
pub enum TorrServerAction {
    Play {
        hash: String,
        name: String,
    },
    SetPaused {
        hash: String,
        name: String,
        paused: bool,
    },
    Remove {
        hash: String,
        name: String,
    },
    Save {
        torrent: TorrentInfo,
        file: TorrentFile,
    },
}

impl App {
    /// The torrent under the cursor, with the cursor clamped to the list
    /// the poller last delivered.
    fn torrserver_selected(&self) -> Option<TorrentInfo> {
        let state = &self.torrserver_state;
        self.torrserver_torrents
            .get(
                state
                    .cursor
                    .min(self.torrserver_torrents.len().saturating_sub(1)),
            )
            .cloned()
    }

    /// The panel's keys. Navigation happens here; everything that needs
    /// the server is returned for the caller to await.
    pub fn torrserver_key(&mut self, key: KeyEvent, vim_keys: bool) -> Option<TorrServerAction> {
        // `a` needs no cursor and opens a dialog instead.
        if key.code == KeyCode::Char('a') {
            self.open_torrserver_magnet_modal();
            return None;
        }

        // A copy, not a borrow: the arms below mutate the panel state, and
        // the poller's list is small enough that a clone costs nothing.
        let torrents = self.torrserver_torrents.clone();
        let state = &mut self.torrserver_state;
        let selected = torrents
            .get(state.cursor.min(torrents.len().saturating_sub(1)))
            .cloned();
        let files: Vec<TorrentFile> = selected
            .as_ref()
            .map(|t| t.files.clone())
            .unwrap_or_default();

        match key.code {
            KeyCode::Char('v') => {
                state.files_open = !state.files_open;
                state.file_cursor = 0;
                None
            }
            KeyCode::Char('j') if vim_keys => {
                move_cursor(state, torrents.len(), &files, 1);
                None
            }
            KeyCode::Char('k') if vim_keys => {
                move_cursor(state, torrents.len(), &files, -1);
                None
            }
            KeyCode::Down => {
                move_cursor(state, torrents.len(), &files, 1);
                None
            }
            KeyCode::Up => {
                move_cursor(state, torrents.len(), &files, -1);
                None
            }
            KeyCode::PageDown => {
                let len = state_len(state, torrents.len(), &files);
                move_cursor(state, torrents.len(), &files, (len / 2).max(1) as i64);
                None
            }
            KeyCode::PageUp => {
                let len = state_len(state, torrents.len(), &files);
                move_cursor(state, torrents.len(), &files, -((len / 2).max(1) as i64));
                None
            }
            KeyCode::Home => {
                if state.files_open {
                    state.file_cursor = 0;
                } else {
                    state.cursor = 0;
                }
                None
            }
            KeyCode::End => {
                if state.files_open {
                    state.file_cursor = files.len().saturating_sub(1);
                } else {
                    state.cursor = torrents.len().saturating_sub(1);
                }
                None
            }
            KeyCode::Enter => selected.map(|t| TorrServerAction::Play {
                hash: t.hash,
                name: t.name,
            }),
            KeyCode::Char('p') => selected.map(|t| TorrServerAction::SetPaused {
                paused: t.is_paused(),
                hash: t.hash,
                name: t.name,
            }),
            KeyCode::Char('d') => {
                let torrent = selected?;
                // Two presses, the same question the download panel asks.
                // The field is read directly rather than through
                // `confirm_remove` because the modal state is borrowed.
                let armed = self.remove_armed;
                self.remove_armed = !armed;
                armed.then_some(TorrServerAction::Remove {
                    hash: torrent.hash,
                    name: torrent.name,
                })
            }
            KeyCode::Char('s') => {
                let torrent = selected?;
                let file = if state.files_open {
                    files
                        .get(state.file_cursor.min(files.len().saturating_sub(1)))
                        .cloned()
                } else {
                    torrent.largest_file().cloned()
                };
                file.map(|file| TorrServerAction::Save { torrent, file })
            }
            _ => None,
        }
    }

    pub(crate) fn render_torrserver_zone(
        &self,
        frame: &mut Frame,
        area: Rect,
        id: ZoneId,
        config: &Config,
    ) {
        let state = &self.torrserver_state;
        let theme = &self.theme;
        let block = self.zone_block(id, config);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        // The keys are on the frame, where every other panel's are.
        self.render_frame(frame, id, area, config);
        if inner.width < 20 || inner.height < 3 {
            return;
        }

        let body = Style::default().fg(theme.main_fg.to_color());
        let label = Style::default().fg(theme.secondary_color());
        let dim = Style::default().fg(theme.div_line.to_color());
        let selected_style = Style::default()
            .fg(theme.hi_fg.to_color())
            .add_modifier(Modifier::BOLD);

        let mut lines: Vec<Line> = Vec::new();
        if self.torrserver_torrents.is_empty() {
            lines.push(Line::from(Span::styled(
                "Nothing to list -- is TorrServer running?",
                body,
            )));
            lines.push(Line::from(Span::styled(
                self.torrserver_url.to_string(),
                dim,
            )));
        } else {
            let cursor = state
                .cursor
                .min(self.torrserver_torrents.len().saturating_sub(1));
            let row_budget = if state.files_open {
                (inner.height as usize / 2).max(3)
            } else {
                (inner.height as usize).saturating_sub(2)
            };
            let visible = row_budget.saturating_sub(1).max(1);
            let first = cursor.saturating_sub(visible.saturating_sub(1));
            lines.push(self.torrserver_header(inner.width as usize, &label));
            for (offset, torrent) in self.torrserver_torrents[first..]
                .iter()
                .take(visible)
                .enumerate()
            {
                let index = first + offset;
                let style = if index == cursor {
                    selected_style
                } else {
                    body
                };
                lines.push(torrent_line(
                    torrent,
                    index == cursor,
                    inner.width as usize,
                    style,
                ));
            }
        }

        if state.files_open {
            if let Some(torrent) = self.torrserver_selected() {
                lines.push(Line::from(Span::styled(
                    format!("Files of {}", torrent.name),
                    label,
                )));
                if torrent.files.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "TorrServer reports no files for this torrent.",
                        dim,
                    )));
                } else {
                    let budget = (inner.height as usize)
                        .saturating_sub(lines.len())
                        .saturating_sub(2)
                        .max(1);
                    let cursor = state.file_cursor.min(torrent.files.len() - 1);
                    let first = cursor.saturating_sub(budget.saturating_sub(1));
                    for (offset, file) in torrent.files[first..].iter().take(budget).enumerate() {
                        let index = first + offset;
                        let style = if index == cursor {
                            selected_style
                        } else {
                            body
                        };
                        lines.push(Line::from(vec![
                            Span::styled(if index == cursor { "▸ " } else { "  " }, style),
                            Span::styled(
                                format!(
                                    "{:>10}  ",
                                    crate::transmission::human_bytes(file.length.max(0) as u64)
                                ),
                                style,
                            ),
                            Span::styled(
                                crate::ui::downloads_panel::truncate(
                                    &file.path,
                                    (inner.width as usize).saturating_sub(14),
                                ),
                                style,
                            ),
                        ]));
                    }
                }
            }
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn torrserver_header(&self, width: usize, label: &Style) -> Line<'static> {
        let name_width = width.saturating_sub(58).max(10);
        Line::from(vec![
            Span::styled("  ", *label),
            Span::styled(format!("{:<name_width$}", "name"), *label),
            Span::styled(format!("{:>7}", "done"), *label),
            Span::styled(format!(" {:<10}", "status"), *label),
            Span::styled(format!("{:>9}", "size"), *label),
            Span::styled(format!("{:>10}", "down"), *label),
            Span::styled(format!("{:>10}", "up"), *label),
            Span::styled(format!(" {:>5}", "seeds"), *label),
        ])
    }
}

/// How many rows the list that owns the cursor has.
fn state_len(state: &TorrServerState, torrents: usize, files: &[TorrentFile]) -> usize {
    if state.files_open {
        files.len()
    } else {
        torrents
    }
}

/// Move whichever list is showing, wrapping so the end is not a wall.
fn move_cursor(state: &mut TorrServerState, torrents: usize, files: &[TorrentFile], delta: i64) {
    let len = state_len(state, torrents, files);
    if len == 0 {
        return;
    }
    let cursor = if state.files_open {
        &mut state.file_cursor
    } else {
        &mut state.cursor
    };
    *cursor = (*cursor as i64 + delta).rem_euclid(len as i64) as usize;
}

fn torrent_line(
    torrent: &TorrentInfo,
    selected: bool,
    width: usize,
    style: Style,
) -> Line<'static> {
    let name_width = width.saturating_sub(58).max(10);
    Line::from(vec![
        Span::styled(if selected { "▸ " } else { "  " }, style),
        Span::styled(
            format!(
                "{:<name_width$}",
                crate::ui::downloads_panel::truncate(&torrent.name, name_width)
            ),
            style,
        ),
        Span::styled(format!("{:>6.1}%", torrent.progress() * 100.0), style),
        Span::styled(
            format!(
                " {:<10}",
                crate::ui::downloads_panel::truncate(&torrent.status_string, 10)
            ),
            style,
        ),
        Span::styled(
            format!(
                "{:>9}",
                crate::transmission::human_bytes(torrent.total_size.max(0) as u64)
            ),
            style,
        ),
        Span::styled(
            format!(
                "{:>10}",
                crate::transmission::human_speed(torrent.download_speed.max(0.0) as i64)
            ),
            style,
        ),
        Span::styled(
            format!(
                "{:>10}",
                crate::transmission::human_speed(torrent.upload_speed.max(0.0) as i64)
            ),
            style,
        ),
        Span::styled(
            format!(
                " {:>2}/{:<2}",
                torrent.connected_seeders.max(0),
                torrent.total_peers.max(0)
            ),
            style,
        ),
    ])
}
