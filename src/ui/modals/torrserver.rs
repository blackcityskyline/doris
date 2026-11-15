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
use crate::ui::theme::Theme;
use crate::ui::view::{App, Modal};

/// Which list the cursor is in: the torrents, or the selected one's files.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TorrServerState {
    pub cursor: usize,
    pub file_cursor: usize,
    /// `v`: the file list of the selected torrent, open under it.
    pub files_open: bool,
}

/// The details modal's poster: fetched when the modal opens, decoded the
/// first time it is drawn (only then is the terminal's protocol known).
///
/// The decoded picture itself lives on `App` (`poster_protocol`), because
/// `ratatui-image`'s render state is neither `Clone` nor `PartialEq` and
/// this struct is part of a `Modal` that has to be both.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Poster {
    /// Set when the torrent carries a picture URL and the answer is still
    /// on its way. A torrent without one never says "loading" -- there is
    /// nothing to wait for.
    pub pending: bool,
    /// The bytes the fetch answered with, until the first draw decodes
    /// them and drops them.
    pub bytes: Option<Vec<u8>>,
    pub error: Option<String>,
}

/// One TorrServer torrent, opened by `i`: the fields its web UI shows and
/// the file list it holds. A modal rather than a zone because there is
/// nothing to act on in it -- the panel's own keys still act on the row.
#[derive(Clone, Debug, PartialEq)]
pub struct TorrServerDetailState {
    pub torrent: TorrentInfo,
    pub poster: Poster,
}

impl TorrServerDetailState {
    pub fn new(torrent: TorrentInfo) -> Self {
        Self {
            // `pending` only when there is something to wait for; a row
            // with no `poster` opens as a details page and nothing else.
            poster: Poster {
                pending: !torrent.poster.trim().is_empty(),
                ..Poster::default()
            },
            torrent,
        }
    }
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
    /// `l`: write the whole torrent's m3u to the download directory, the
    /// same file the web UI's playlist button downloads.
    SavePlaylist {
        hash: String,
        name: String,
    },
    /// `i`: show the server's own fields, poster included.
    Details {
        torrent: TorrentInfo,
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
            // A playlist is per torrent, not per file: one answer covers a
            // season pack, and it is the only thing here that can be saved
            // without the server staying up.
            KeyCode::Char('l') => selected.map(|t| TorrServerAction::SavePlaylist {
                hash: t.hash,
                name: t.name,
            }),
            KeyCode::Char('i') => selected.map(|t| TorrServerAction::Details { torrent: t }),
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

    /// `i`: the torrent as the server sees it, poster included.
    ///
    /// `&mut self` because the image protocol keeps its own render state
    /// between frames -- and because decoding the poster happens here, on
    /// the first draw, when the terminal's protocol is finally known.
    pub fn render_torrserver_detail_modal(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        config: &Config,
    ) {
        let theme = &self.theme;
        let popup = crate::ui::view::centered_rect(80, 80, area);
        frame.render_widget(Clear, popup);
        let block = self
            .modal_block(theme.primary_color(), config)
            .title(Span::styled(
                " TorrServer details ",
                Style::default().fg(theme.primary_color()),
            ));
        let inner = block.inner(popup);
        frame.render_widget(block, popup);
        if inner.width < 20 || inner.height < 4 {
            return;
        }
        let Modal::TorrServerDetail(state) = &mut self.modal else {
            return;
        };

        // The picture is the right half when there is one to draw; without
        // a poster URL the text takes the whole box rather than leaving a
        // permanently empty column.
        let has_poster = !state.torrent.poster.trim().is_empty();
        let columns = if has_poster {
            Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(inner)
                .to_vec()
        } else {
            vec![inner]
        };

        let label = Style::default()
            .fg(theme.secondary_color())
            .add_modifier(Modifier::BOLD);
        let value = Style::default().fg(theme.main_fg.to_color());
        let dim = Style::default().fg(theme.inactive_fg.to_color());
        let torrent = &state.torrent;

        let mut lines: Vec<Line> = vec![
            Line::from(vec![
                Span::styled("Title:    ", label),
                Span::styled(torrent.name.clone(), value),
            ]),
            Line::from(vec![
                Span::styled("Hash:     ", label),
                Span::styled(torrent.hash.clone(), value),
            ]),
            Line::from(vec![
                Span::styled("Category: ", label),
                Span::styled(
                    if torrent.category.is_empty() {
                        "-".to_string()
                    } else {
                        torrent.category.clone()
                    },
                    value,
                ),
            ]),
            Line::from(vec![
                Span::styled("Size:     ", label),
                Span::styled(
                    crate::transmission::human_bytes(torrent.total_size.max(0) as u64),
                    value,
                ),
            ]),
            Line::from(vec![
                Span::styled("Done:     ", label),
                Span::styled(
                    format!(
                        "{:.1}%  ({} of {})",
                        torrent.progress() * 100.0,
                        crate::transmission::human_bytes(torrent.loaded_size.max(0) as u64),
                        crate::transmission::human_bytes(torrent.total_size.max(0) as u64),
                    ),
                    value,
                ),
            ]),
            Line::from(vec![
                Span::styled("Status:   ", label),
                Span::styled(torrent.status_string.clone(), value),
            ]),
            Line::from(vec![
                Span::styled("Speed:    ", label),
                Span::styled(
                    format!(
                        "{} down / {} up",
                        crate::transmission::human_speed(torrent.download_speed.max(0.0) as i64),
                        crate::transmission::human_speed(torrent.upload_speed.max(0.0) as i64),
                    ),
                    value,
                ),
            ]),
            Line::from(vec![
                Span::styled("Peers:    ", label),
                Span::styled(
                    format!(
                        "{} seeds / {} peers",
                        torrent.connected_seeders.max(0),
                        torrent.total_peers.max(0)
                    ),
                    value,
                ),
            ]),
            Line::from(vec![
                Span::styled("Added:    ", label),
                Span::styled(crate::ui::draw::added(torrent.timestamp), value),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                format!("Files ({}):", torrent.files.len()),
                label,
            )),
        ];
        if torrent.files.is_empty() {
            lines.push(Line::from(Span::styled(
                "  the server reports no files",
                dim,
            )));
        } else {
            // The box clips what does not fit; `v` in the panel is the
            // cursor-driven list, so this one only has to be complete for
            // the torrents that do fit -- a film's one or two files.
            for file in &torrent.files {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(
                            "{:>10}  ",
                            crate::transmission::human_bytes(file.length.max(0) as u64)
                        ),
                        dim,
                    ),
                    Span::styled(file.path.clone(), value),
                ]));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("Esc: close", dim)));
        frame.render_widget(Paragraph::new(lines), columns[0]);

        if has_poster {
            let hash = state.torrent.hash.clone();
            render_poster(
                frame,
                columns[1],
                self.picker.as_ref(),
                &hash,
                &mut state.poster,
                &mut self.poster_protocol,
                theme,
            );
        }
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

/// The poster half of the details modal: the picture once the fetch and
/// the decode have both landed, and what it is waiting for until then.
///
/// `protocol` is the one decoded picture on `App`; `hash` says which
/// torrent it belongs to, so reopening the modal on another row replaces it
/// instead of drawing the last one's cover.
fn render_poster(
    frame: &mut Frame,
    area: Rect,
    picker: Option<&ratatui_image::picker::Picker>,
    hash: &str,
    poster: &mut Poster,
    protocol: &mut Option<(String, ratatui_image::protocol::StatefulProtocol)>,
    theme: &Theme,
) {
    // Decode once, when both the bytes and the terminal's protocol are
    // known. The bytes are then dropped: the protocol holds the pixels.
    let stale = protocol.as_ref().map(|(h, _)| h != hash).unwrap_or(false);
    if stale {
        *protocol = None;
    }
    if protocol.is_none() {
        if let (Some(picker), Some(bytes)) = (picker, poster.bytes.as_ref()) {
            match image::load_from_memory(bytes) {
                Ok(image) => {
                    *protocol = Some((hash.to_string(), picker.new_resize_protocol(image)));
                }
                Err(e) => poster.error = Some(format!("poster is not an image: {e}")),
            }
            poster.bytes = None;
        }
    }

    if let Some((_, protocol)) = protocol.as_mut() {
        frame.render_stateful_widget(ratatui_image::StatefulImage::default(), area, protocol);
        return;
    }
    let text = match (&poster.error, poster.pending) {
        (Some(err), _) => format!("poster: {err}"),
        (None, true) => "loading poster...".to_string(),
        (None, false) => "no poster".to_string(),
    };
    let style = Style::default().fg(theme.inactive_fg.to_color());
    frame.render_widget(
        Paragraph::new(text).style(style).wrap(Wrap { trim: true }),
        area,
    );
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
