//! Drawing the TUI: the search bar, the four zones, the frames around
//! them, the three detail takeovers and the modal dispatcher.
//!
//! Split from `ui/app.rs` because everything here reads the App and
//! writes nothing -- the state, and the decisions about it, are in
//! `app.rs`, and a method that only reads has no business sitting next
//! to the ones that mutate.

use super::layout::{FrameButton, ZoneId, SEARCH_BAR_HEIGHT};
use super::theme::Theme;
use super::view::{source_badge, source_rows, App, AppState, Modal, SourceRow, SOURCE_BADGE_WIDTH};
use crate::config::Config;
use crate::sources::orchestrator::SourceStatus;
use ratatui::prelude::*;
use ratatui::widgets::*;

impl App {
    /// The block every zone frame is built from: themed, bordered, and titled with the zone's
    /// own number and label.
    fn zone_block(&self, id: ZoneId, config: &Config) -> Block<'static> {
        self.themed_block(
            super::layout::zone_border_color(id, self.zones.focused, &self.theme),
            config,
        )
        .title(super::layout::zone_title(
            id,
            &self.theme,
            id == self.zones.focused,
        ))
    }

    /// Border+background styling for the four main zone panels, respecting the "Rounded
    /// corners", "Theme background" and "Show boxes" Options toggles.
    pub(crate) fn themed_block(&self, border_color: Color, config: &Config) -> Block<'static> {
        self.themed_block_with_borders(
            border_color,
            config,
            if config.show_boxes {
                Borders::ALL
            } else {
                Borders::NONE
            },
        )
    }

    pub(super) fn themed_block_with_borders(
        &self,
        border_color: Color,
        config: &Config,
        borders: Borders,
    ) -> Block<'static> {
        let border_color = self.resolve_color(border_color, config);
        let border_type = if config.rounded_corners && !config.false_tty {
            BorderType::Rounded
        } else {
            BorderType::Plain
        };
        let mut block = Block::default()
            .borders(borders)
            .border_type(border_type)
            .border_style(Style::default().fg(border_color));
        // Off, the block carries no `bg` so the `Clear` drawn before it
        if config.theme_background {
            block = block.style(
                Style::default().bg(self.resolve_color(self.theme.main_bg.to_color(), config)),
            );
        }
        block
    }

    /// Degrade an RGB color per the "Truecolor"/"False tty" toggles; see
    /// `theme::degrade_color`.
    fn resolve_color(&self, color: Color, config: &Config) -> Color {
        if config.false_tty {
            super::theme::degrade_color(color, false)
        } else if !config.truecolor {
            super::theme::degrade_color(color, true)
        } else {
            color
        }
    }

    /// Draw the main view.
    pub fn render(&mut self, frame: &mut Frame, config: &Config) {
        let area = frame.area();
        // The app first, the menu's glyphs over it. The reference builds
        // the frame and then prints `Global::overlay` on top of it
        // -- the overlay is drawn after the frame, so the panels stay readable
        // behind the menu.
        self.render_main_view(frame, area, config);
        if self.show_menu {
            super::menu::render_menu(frame, area, &self.menu, &self.theme);
        }
    }

    fn render_main_view(&mut self, frame: &mut Frame, area: Rect, config: &Config) {
        self.zones.update_areas(area);

        // A zone with no detail view (`Trackers`) simply has nothing to
        let detail = self.detail_view.filter(|id| id.detail_key().is_some());
        if let Some(view) = detail {
            match view {
                ZoneId::Log => self.render_full_log(frame, area, config),
                ZoneId::Torrent => self.render_detail_torrent(frame, area, config),
                _ => self.render_detail_results(frame, area, config),
            }
        } else {
            self.render_search_bar(frame, area, config);

            for zone_id in ZoneId::all() {
                let zone_area = self.zones.get_area(*zone_id);
                if zone_area.width == 0 || zone_area.height == 0 {
                    continue;
                }

                match zone_id {
                    ZoneId::Results => self.render_results_zone(frame, zone_area, *zone_id, config),
                    ZoneId::Torrent => self.render_torrent_zone(frame, zone_area, *zone_id, config),
                    ZoneId::Log => self.render_log_zone(frame, zone_area, *zone_id, config),
                    ZoneId::Trackers => {
                        self.render_trackers_zone(frame, zone_area, *zone_id, config)
                    }
                }
            }
        }

        if self.modal != Modal::None {
            self.render_modal(frame, area, config);
        }
    }

    fn render_search_bar(&self, frame: &mut Frame, area: Rect, config: &Config) {
        let bar_area = Rect::new(area.x, area.y, area.width, SEARCH_BAR_HEIGHT);

        // A label, not a keybind cheat-sheet: where the keys live is
        let filter_on = self.zones.filter_mode || !self.zones.filter_input.is_empty();
        // While `f` is open the text lives in the box, so the border
        let title = match (self.input_mode, self.zones.filter_mode, filter_on) {
            (false, true, _) => "filter".to_string(),
            (false, false, true) => format!("filter: {}", self.zones.filter_input),
            _ => "Search".to_string(),
        };
        // The `S` of Search is a keybind glyph like any other, so it
        // takes `hi_fg`: the same colour as the `f` of `filter`, the
        // zone's digit and the panel's detail-view letter.
        let title_line = if title == "Search" {
            let word = Style::default().fg(self.theme.title.to_color());
            let hot = Style::default()
                .fg(self.theme.hi_fg.to_color())
                .add_modifier(Modifier::BOLD);
            Line::from(vec![Span::styled("S", hot), Span::styled("earch", word)])
        } else {
            Line::from(Span::styled(
                title.clone(),
                Style::default().fg(self.theme.primary_color()),
            ))
        };

        // The box holds whichever string is being edited: the query in
        let editing: &str = if self.zones.filter_mode {
            self.zones.filter_input.as_str()
        } else {
            self.search_input.as_str()
        };

        let input_border = self
            .themed_block(
                if self.input_mode || self.zones.filter_mode {
                    // The box under the cursor takes the same accent a
                    self.theme.primary_color()
                } else {
                    self.theme.div_line.to_color()
                },
                config,
            )
            .title(title_line);

        let inner = input_border.inner(bar_area);
        let input = Paragraph::new(editing)
            .block(input_border)
            .style(Style::default().fg(self.theme.main_fg.to_color()));

        frame.render_widget(input, bar_area);

        // A text field without a caret is a text field you type into
        if (self.input_mode || self.zones.filter_mode)
            && self.modal == Modal::None
            && !self.show_menu
        {
            // The caret's own column, which is not the end of the text once
            // the arrows move it.
            let col = if self.input_mode {
                self.cursor_column()
            } else {
                editing.chars().count()
            } as u16;
            frame.set_cursor_position((inner.x + col.min(inner.width.saturating_sub(1)), inner.y));
        }
    }

    fn render_results_zone(&self, frame: &mut Frame, area: Rect, id: ZoneId, config: &Config) {
        let block = self.zone_block(id, config);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // One row inside the border: the table. The category row that
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0)]) // results table
            .split(inner);

        // --- results table ---------------------------------------------------
        let Some(table) = self.results_table(6, 8, 8, 20) else {
            // The block is already on screen, so this only fills its
            self.render_results_placeholder(frame, inner);
            self.render_frame(frame, id, area, config);
            return;
        };
        frame.render_stateful_widget(table, chunks[0], &mut self.results_cursor());

        // The keybind legend moved onto the frame, so the panel
        self.render_frame(frame, id, area, config);
    }

    /// The Trackers panel: the `all` master switch on top, then one row per registered source,
    /// `[x]`/`[ ]` showing whether the search asks it.
    fn render_trackers_zone(&self, frame: &mut Frame, area: Rect, id: ZoneId, config: &Config) {
        let block = self.zone_block(id, config);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // The status reads as a column only if every id is padded to
        let roster = source_rows();
        let id_width = roster
            .iter()
            .map(|r| r.id().chars().count())
            .max()
            .unwrap_or(3);
        let rows: Vec<Line> = roster
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                let checked = row.is_checked(config);
                let mark = if checked { "x" } else { " " };
                let mut style = if !row.is_implemented() {
                    Style::default().fg(self.theme.inactive_fg.to_color())
                } else {
                    Style::default().fg(self.theme.main_fg.to_color())
                };
                let cursor = index == self.sources_cursor;
                if cursor {
                    style = self.theme.selection_style();
                }
                // The mark is the one character that states the row's state, so it
                // wears the panel's own accent rather than the body
                // colour the source id is drawn in -- and rather than the
                // keybind accent, which is for keys.
                let mut spans = vec![
                    Span::styled("[", style),
                    Span::styled(
                        mark,
                        if cursor {
                            style
                        } else {
                            Style::default().fg(self.theme.primary_color())
                        },
                    ),
                    Span::styled(format!("] {:<width$}", row.id(), width = id_width), style),
                ];
                if let SourceRow::One(source_id) = row {
                    if let Some(status) = self.source_status.get(source_id) {
                        let status_style = if cursor {
                            self.theme.selection_style()
                        } else {
                            source_status_style(status, &self.theme)
                        };
                        spans.push(Span::styled(
                            format!(" {}", source_status_text(status)),
                            status_style,
                        ));
                    }
                }
                // A planned source is listed -- so the next one is
                if !row.is_implemented() {
                    spans.push(Span::styled(" (planned)", style));
                }
                Line::from(spans)
            })
            .collect();

        // The panel can be hidden (`1`-`4` again) and the terminal can
        let visible = inner.height as usize;
        let offset = self
            .sources_cursor
            .saturating_sub(visible.saturating_sub(1));
        let shown: Vec<Line> = rows.into_iter().skip(offset).take(visible).collect();
        frame.render_widget(Paragraph::new(shown), inner);

        self.render_frame(frame, id, area, config);
    }

    fn render_torrent_zone(&self, frame: &mut Frame, area: Rect, id: ZoneId, config: &Config) {
        // The panel is the list, always. It used to fall back to a
        // single-torrent readout when the daemon held nothing, so the
        // panel's identity depended on whether a download happened to
        // exist: the same key meant two different things on two launches,
        // and a daemon that simply is not running looked like the old
        // panel rather than like a daemon that is not running.
        self.render_downloads(frame, area, id, config);
    }

    /// The streaming torrent's live line, shown above the list while one
    /// is active.
    ///
    /// The old panel showed exactly this and nothing else, which is why
    /// anything else in the daemon was invisible. It stays, as one line:
    /// the streaming server is a different service from the downloading
    /// daemon and its state has nowhere else to go.
    ///
    /// The hash is on this line and not only in the detail view because
    /// the detail view is about a download now: the streamed torrent's
    /// hash -- the one thing that identifies it -- was reachable nowhere
    /// once the daemon held anything at all.
    fn render_stream_line(&self, width: usize) -> Option<Line<'_>> {
        let s = &self.torrent_status;
        // Not gated on the hash alone. Between pressing Enter and TorrServer
        // answering there is a window with a state and no hash, and that is
        // the window in which the panel must say something -- a launch that
        // shows nothing looks like an app that ignored the key.
        if s.hash.is_empty() && self.state != AppState::Streaming {
            return None;
        }
        let label = Style::default().fg(self.theme.secondary_color());
        let value = Style::default().fg(self.theme.main_fg.to_color());
        // "starting" means the hash has not come back yet, not that the
        // app state happens to be Streaming: a launch that TorrServer has
        // already answered for is running, and calling it starting again
        // is the same mistake as drawing it as empty.
        let state: &str = if self.state == AppState::Streaming && s.hash.is_empty() {
            "starting"
        } else {
            &s.status
        };
        let mut spans = vec![
            Span::styled("stream ", label),
            Span::styled(format!("{:>3.0}%", s.progress * 100.0), value),
            Span::styled("  ", value),
            Span::styled(state, value),
            Span::styled("  ", value),
            Span::styled(&s.title, label),
        ];
        // What is left over goes to the hash, and what it leaves over is
        // dropped rather than wrapped: a second line would look like a
        // second torrent.
        let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let room = width.saturating_sub(used + 2);
        if !s.hash.is_empty() && room >= 12 {
            spans.push(Span::styled("  ", value));
            spans.push(Span::styled(
                super::torrents_panel::truncate(&s.hash, room),
                value,
            ));
        }
        Some(Line::from(spans))
    }

    #[allow(dead_code)]
    fn render_torrent_status_legacy(
        &self,
        frame: &mut Frame,
        area: Rect,
        id: ZoneId,
        config: &Config,
    ) {
        let s = &self.torrent_status;

        let progress_pct = (s.progress * 100.0) as u32;
        let bar_width = (area.width as usize).saturating_sub(4).min(50);

        let dl_speed = format_bytes(s.download_speed);
        let ul_speed = format_bytes(s.upload_speed);
        let dl_total = format_bytes(s.downloaded);
        let total = format_bytes(s.total_size);

        let status_display = if self.torrent_paused && !s.hash.is_empty() {
            format!("{} (paused)", s.status)
        } else {
            s.status.clone()
        };

        let history: Vec<f64> = self.progress_history.iter().copied().collect();
        let sparkline =
            super::widgets::graph::render_sparkline(&history, bar_width, &config.graph_symbol);

        // TorrServer has not named the torrent yet: from Enter to the
        let label = Style::default().fg(self.theme.secondary_color());
        let value = Style::default().fg(self.theme.main_fg.to_color());

        let header = if s.hash.is_empty() && self.state == AppState::Streaming {
            Line::from(vec![
                Span::styled("Status: ", label),
                Span::styled("Starting stream...", value),
            ])
        } else {
            Line::from(vec![
                Span::styled("Hash: ", label),
                Span::styled(s.hash.as_str(), value),
                Span::styled("  Status: ", label),
                Span::styled(status_display, value),
            ])
        };

        let mut lines = vec![
            header,
            Line::from(vec![
                Span::styled("Progress: ", label),
                Span::styled(
                    format!("{} {}%", sparkline, progress_pct),
                    if progress_pct >= 100 {
                        Style::default().fg(self.theme.primary_color())
                    } else {
                        value
                    },
                ),
            ]),
            Line::from(vec![
                Span::styled("DL: ", label),
                Span::styled(&dl_speed, value),
                Span::styled("  ", value),
                Span::styled("UL: ", label),
                Span::styled(&ul_speed, value),
            ]),
            Line::from(vec![
                Span::styled("Downloaded: ", label),
                Span::styled(&dl_total, value),
                Span::styled(" / ", value),
                Span::styled(&total, value),
                Span::styled("  Seeds: ", label),
                Span::styled(s.seeds.to_string(), value),
                Span::styled("  Peers: ", label),
                Span::styled(s.peers.to_string(), value),
            ]),
        ];

        // The armed removal takes the last line rather than replacing one:
        if let Some(prompt) = self.remove_prompt() {
            lines.push(Line::from(Span::styled(
                prompt,
                Style::default()
                    .fg(self.theme.error_color())
                    .add_modifier(Modifier::BOLD),
            )));
        }

        let block = self.zone_block(id, config);
        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, area);

        // "p: pause/resume d: remove" is gone from the body: those two
        self.render_frame(frame, id, area, config);
    }

    /// The Torrents panel: a one-line summary over a table of downloads,
    /// both borrowed from qbittorrent-tui's arrangement -- three summary
    /// sections squashed into the one row a zone usually has room for, and
    /// a table whose columns are dropped from the tail when the panel is
    /// narrow so that the name always survives.
    fn render_downloads(&self, frame: &mut Frame, area: Rect, id: ZoneId, config: &Config) {
        let block = self.zone_block(id, config);
        let inner = block.inner(area);
        let inner_width = inner.width as usize;
        let prompt = self.remove_prompt_line();

        // The same three boxes the `T` view draws, by the same call, with the
        // same fallback to a single line when there is no room for them.
        //
        // The panel had the numbers flat on one line while the full view boxed
        // the identical numbers in three frames: one set of numbers drawn two
        // ways, and the panel was the one that read as a wall of text. The
        // table stays unframed here -- the zone's own frame is the frame
        // around it, and a zone is too small to be framed twice.
        //
        // "Room for them" means room for the table too: the boxes are only
        // drawn when what is left still holds the header, the streaming line,
        // at least one download and the armed question. A panel whose boxes
        // push the question off the screen is worse than a panel with its
        // numbers on one line.
        let overhead = 2 + usize::from(prompt.is_some()); // header + stream line
        let widths = super::torrents_panel::section_widths(inner_width);
        let framed =
            widths.is_some() && (inner.height as usize) > SECTION_HEIGHT as usize + overhead;
        let sections_height = if framed { SECTION_HEIGHT } else { 0 };

        // Rows the table does not get: the boxes, its own header, the
        // streaming line above it and the question. Everything else is a
        // download, and downloads are what the panel is for.
        let visible = (inner.height as usize)
            .saturating_sub(sections_height as usize + overhead)
            .max(1);
        let parts = self.downloads_parts(inner_width, visible, false);

        let body = Style::default().fg(self.theme.main_fg.to_color());
        let mut lines = Vec::new();
        if !framed {
            lines.push(Line::from(Span::styled(parts.summary.one_line(), body)));
        }
        lines.extend(parts.stream);
        lines.extend(parts.table);
        lines.extend(prompt);

        // The frame on the whole zone, the content in what is left under the
        // boxes: padding the paragraph with blank lines instead would make
        // the height of the boxes a guess about where the table starts.
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(lines),
            Rect {
                y: inner.y + sections_height,
                height: inner.height.saturating_sub(sections_height),
                ..inner
            },
        );
        if let Some(widths) = widths.filter(|_| framed) {
            self.render_sections(
                frame,
                Rect {
                    x: inner.x,
                    y: inner.y,
                    width: inner.width,
                    height: SECTION_HEIGHT,
                },
                &parts.summary,
                &widths,
                config,
            );
        }
        self.render_frame(frame, id, area, config);
    }

    /// The panel's own content, in its three parts: the numbers, whatever
    /// is being streamed, and the table.
    ///
    /// One function, two renderers. The zone has one line for the numbers
    /// and no room for frames inside its own; the `T` view has the whole
    /// terminal and draws the same numbers as three boxes and the same
    /// table as a fourth. Sharing them is what keeps the two from drifting
    /// into showing different things -- which is how the detail view came
    /// to be a list of `Label: value` lines beside a table.
    fn downloads_parts(&self, inner_width: usize, visible: usize, marker: bool) -> Downloads<'_> {
        use super::torrents_panel as panel;

        let label = Style::default().fg(self.theme.secondary_color());
        let summary = panel::summary(&self.downloads, self.free_space, self.daemon_reachable);
        // A marker column, two columns wide. Bold and a highlight colour are
        // what the cursor row is drawn with everywhere else, and on a table
        // of fourteen near-identical rows that is not enough to see where
        // you are -- which is the same thing as not having a cursor.
        let marked = if marker { 2 } else { 0 };
        let inner_width = inner_width.saturating_sub(marked);
        let stream = self.render_stream_line(inner_width);
        let mut table: Vec<Line> = Vec::new();

        let Some(plan) = panel::plan(inner_width) else {
            table.push(Line::from(Span::styled(
                format!("{} torrents (panel too narrow)", self.downloads.len()),
                label,
            )));
            return Downloads {
                summary,
                stream,
                table,
            };
        };

        // One header, then as many rows as the remaining height allows. The
        // cursor is kept in view rather than scrolled: the list is short,
        // and a panel that scrolls a handful of rows to show a cursor is
        // noisier than one that does not.
        let first = self
            .download_cursor
            .saturating_sub(visible.saturating_sub(1))
            .min(self.downloads.len().saturating_sub(1));

        table.push(Line::from(Span::styled(
            panel::header(&plan),
            Style::default().fg(self.theme.div_line.to_color()),
        )));
        let hot = Style::default()
            .fg(self.theme.hi_fg.to_color())
            .add_modifier(Modifier::BOLD);
        let body = Style::default().fg(self.theme.main_fg.to_color());
        for (offset, row) in self.downloads[first..].iter().take(visible).enumerate() {
            let idx = first + offset;
            let selected = idx == self.download_cursor;
            let text = match marker {
                true if selected => format!("▸ {}", panel::row(row, &plan)),
                true => format!("  {}", panel::row(row, &plan)),
                false => panel::row(row, &plan),
            };
            table.push(Line::from(Span::styled(
                text,
                if selected { hot } else { body },
            )));
        }
        if self.downloads.len() > first + visible {
            table.push(Line::from(Span::styled(
                format!("... {} more", self.downloads.len() - first - visible),
                label,
            )));
        }
        Downloads {
            summary,
            stream,
            table,
        }
    }

    /// The armed removal question, when there is one.
    fn remove_prompt_line(&self) -> Option<Line<'static>> {
        self.remove_prompt().map(|prompt| {
            Line::from(Span::styled(
                prompt,
                Style::default()
                    .fg(self.theme.error_color())
                    .add_modifier(Modifier::BOLD),
            ))
        })
    }

    fn render_log_zone(&self, frame: &mut Frame, area: Rect, id: ZoneId, config: &Config) {
        let visible = (area.height as usize).saturating_sub(2);
        let offset = self.log_scroll.saturating_sub(visible);

        let visible_logs: Vec<Line> = self
            .logs
            .iter()
            .skip(offset)
            .take(visible)
            .map(|l| Line::from(l.as_str()))
            .collect();

        let log_panel = Paragraph::new(visible_logs).block(self.zone_block(id, config));

        frame.render_widget(log_panel, area);
        // The "(n/m)" scroll position moved from the title onto the
        self.render_frame(frame, id, area, config);
    }

    fn render_full_log(&self, frame: &mut Frame, area: Rect, config: &Config) {
        let total = self.detail_logs.len();
        let visible = (area.height as usize).saturating_sub(2);
        let scroll = self.detail_log_scroll.saturating_sub(visible);

        let lines: Vec<Line> = self
            .detail_logs
            .iter()
            .skip(scroll)
            .take(visible)
            .map(|l| Line::from(Span::styled(l.as_str(), detail_log_style(l, &self.theme))))
            .collect();

        let title = format!(
            " Detailed Log ({}/{}) [L/Esc] close [j/k] scroll ",
            scroll + visible.min(total),
            total
        );

        let log_panel = Paragraph::new(lines).block(
            self.themed_block(self.theme.primary_color(), config)
                .title(Span::styled(
                    title,
                    Style::default().fg(self.theme.primary_color()),
                )),
        );

        frame.render_widget(log_panel, area);
    }

    /// The Torrent detail view (`T`): every fact about one download,
    /// given the whole frame.
    ///
    /// It describes the row under the Torrents panel's cursor, because that
    /// is what the panel shows. It used to describe `torrent_status`, which
    /// is the *streaming* server's one torrent -- so the panel became a
    /// list and `T` kept opening a different service's view of something
    /// else entirely, under the same key. The streaming state still has
    /// its place: it is the `stream` line on the panel, and this view falls
    /// back to it when nothing is being downloaded, which is the only case
    /// where it is the subject rather than a footnote.
    fn render_detail_torrent(&self, frame: &mut Frame, area: Rect, config: &Config) {
        let facts: Vec<Line> = match self.downloads.get(self.download_cursor) {
            Some(row) => self.download_detail_lines(row, config),
            None => self.stream_detail_lines(config),
        };

        let title = Span::styled(
            // `T` and not `T/Esc`: Esc opens the menu over this view, so
            // promising to close it here would be a lie about the key.
            " Torrent detail [T close, Esc menu] ",
            Style::default().fg(self.theme.primary_color()),
        );
        let block = self
            .themed_block(self.theme.primary_color(), config)
            .title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Four boxes, because one undifferentiated block of text is what
        // this view was: three questions asked together on one row, then a
        // table, then a list of facts, with nothing saying which is which.
        // The boxes are the whole difference -- qbittorrent-tui draws the
        // same four and its numbers are the same numbers.
        let inner_width = inner.width as usize;
        let sections = super::torrents_panel::section_widths(inner_width);
        let prompt = self.remove_prompt_line();

        // Budgeted from the inside out, every part derived from the one
        // below it rather than from the frame: a rect that reaches past
        // `inner` is a panic in the buffer, not a frame drawn slightly
        // wrong. The table is the part that loses rows -- it is a list, and
        // a list with fewer rows is still a list.
        let wanted_facts = (facts.len() as u16).saturating_add(2);
        let framed = sections.is_some() && inner.height >= wanted_facts + SECTION_HEIGHT + 7;
        let sections_height = if framed { SECTION_HEIGHT } else { 0 };
        // The facts box whole or not at all: a frame too short to hold one
        // fact line is a caption over nothing, and the rows it ate are rows
        // of the table the user came to this view for.
        let room = inner.height.saturating_sub(sections_height);
        let facts_height = if room >= wanted_facts + 3 {
            wanted_facts
        } else {
            0
        };
        let table_height = room.saturating_sub(facts_height).max(1);

        let mut y = inner.y;
        let mut table_lines: Vec<Line> = Vec::new();
        // Four for the box, the header and the remove prompt, one spare: a control
        // row that gets pushed out of the frame is a control the user cannot
        // see, which is where this all started.
        let visible = (table_height as usize).saturating_sub(5).max(1);
        let parts = self.downloads_parts(inner_width, visible, true);
        table_lines.extend(parts.stream);

        match sections {
            Some(widths) if framed => {
                self.render_sections(
                    frame,
                    Rect {
                        x: inner.x,
                        y,
                        width: inner.width,
                        height: SECTION_HEIGHT,
                    },
                    &parts.summary,
                    &widths,
                    config,
                );
                y += SECTION_HEIGHT;
            }
            _ => {
                // Truncated rather than clipped: a line cut at the border
                // ends mid-word with nothing saying anything was lost.
                let body = Style::default().fg(self.theme.main_fg.to_color());
                table_lines.push(Line::from(Span::styled(
                    super::torrents_panel::truncate(&parts.summary.one_line(), inner_width),
                    body,
                )));
            }
        }

        table_lines.extend(parts.table);
        if let Some(prompt) = prompt {
            table_lines.push(prompt);
        }
        self.render_box(
            frame,
            Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: table_height,
            },
            " downloads ",
            table_lines,
            config,
        );

        if facts_height >= 3 {
            self.render_box(
                frame,
                Rect {
                    x: inner.x,
                    y: y + table_height,
                    width: inner.width,
                    height: facts_height,
                },
                self.detail_box_title(),
                facts,
                config,
            );
        }

        // The keys, on the bottom border of the downloads box: the box whose
        // rows they act on.
        self.render_buttons_on_bottom_border(
            frame,
            Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: table_height,
            },
            &super::layout::detail_buttons(),
            config,
        );
    }

    /// The three summary boxes, side by side.
    fn render_sections(
        &self,
        frame: &mut Frame,
        area: Rect,
        summary: &super::torrents_panel::Summary,
        widths: &[usize; 3],
        config: &Config,
    ) {
        let body = Style::default().fg(self.theme.main_fg.to_color());
        let sections: [(&str, Vec<Line>, usize); 3] = [
            (
                " status ",
                vec![
                    Line::from(Span::styled(summary.status.clone(), body)),
                    Line::from(Span::styled(summary.speeds.clone(), body)),
                ],
                widths[0],
            ),
            (
                " active ",
                vec![
                    Line::from(Span::styled(summary.active.clone(), body)),
                    Line::from(Span::styled(summary.session.clone(), body)),
                ],
                widths[1],
            ),
            (
                " free space ",
                // One line, like the reference: a second would be a second
                // thing to read for no more information.
                vec![Line::from(Span::styled(summary.free.clone(), body))],
                widths[2],
            ),
        ];
        let mut x = area.x;
        for (title, lines, width) in sections {
            let left = (x - area.x) as usize;
            let w = width.min((area.width as usize).saturating_sub(left)) as u16;
            if w < 4 {
                break;
            }
            // Clipped at the border otherwise, which cuts a word in half
            // with no mark that anything was lost.
            let lines: Vec<Line> = lines
                .into_iter()
                .map(|line| {
                    let spans: Vec<Span> = line
                        .spans
                        .into_iter()
                        .map(|span| {
                            let style = span.style;
                            Span::styled(
                                super::torrents_panel::truncate(&span.content, w as usize - 2),
                                style,
                            )
                        })
                        .collect();
                    Line::from(spans)
                })
                .collect();
            self.render_box(
                frame,
                Rect {
                    x,
                    y: area.y,
                    width: w,
                    height: area.height,
                },
                title,
                lines,
                config,
            );
            x += w + 1;
        }
    }

    /// One framed box: a title and its lines.
    fn render_box<'a>(
        &self,
        frame: &mut Frame,
        area: Rect,
        title: impl Into<String>,
        lines: Vec<Line<'a>>,
        config: &Config,
    ) {
        let title = Span::styled(
            title.into(),
            Style::default().fg(self.theme.primary_color()),
        );
        let block = self
            .themed_block(self.theme.div_line.to_color(), config)
            .title(title);
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }

    /// The facts box is named after the torrent it describes.
    fn detail_box_title(&self) -> String {
        // Padded like every other title, or the name starts on the corner
        // glyph and reads as part of the border.
        let name = self
            .downloads
            .get(self.download_cursor)
            .map(|row| row.name.as_str())
            .unwrap_or("stream");
        format!(" {name} ")
    }

    /// One download, every fact the daemon reported about it.
    ///
    /// Six facts the table cannot show: where it is being written, when it
    /// was added, the ratio, the ETA, and the per-tracker seeder counts. The
    /// tracker's own name and its seed count are the only way to tell "no
    /// peers" from "one tracker answered and three did not".
    fn download_detail_lines(
        &self,
        row: &crate::ui::view::DownloadRow,
        config: &Config,
    ) -> Vec<Line<'_>> {
        let label = Style::default().fg(self.theme.secondary_color());
        let value = Style::default().fg(self.theme.main_fg.to_color());
        let percent = row.percent();
        let progress_style = if percent >= 100.0 {
            Style::default().fg(self.theme.primary_color())
        } else {
            value
        };

        let mut lines = vec![
            field("Name: ", &row.name, &label, &value),
            field("Hash: ", &row.hash, &label, &value),
            field("State: ", row.state(), &label, &value),
            Line::from(vec![
                Span::styled("Progress: ", label),
                Span::styled(format!("{percent:.0}%"), progress_style),
                Span::styled(
                    format!(
                        "   {} of {}",
                        bytes(row.bytes_done().max(0) as u64),
                        bytes(row.total_size.max(0) as u64)
                    ),
                    value,
                ),
            ]),
            Line::from(vec![
                Span::styled("DL: ", label),
                Span::styled(speed(row.download_speed), value),
                Span::styled("  ", value),
                Span::styled("UL: ", label),
                Span::styled(speed(row.upload_speed), value),
                Span::styled("  ETA: ", label),
                Span::styled(row.eta_text().unwrap_or_else(|| "--".into()), value),
            ]),
            Line::from(vec![
                Span::styled("Ratio: ", label),
                Span::styled(
                    row.ratio()
                        .map(|r| format!("{r:.2}"))
                        .unwrap_or_else(|| "--".into()),
                    value,
                ),
                Span::styled("  Seeds: ", label),
                Span::styled(row.seeds.to_string(), value),
                Span::styled("  Peers: ", label),
                Span::styled(row.peers.to_string(), value),
            ]),
            // The limit is here because `+` and `-` change it and nothing
            // else on screen would: a control with no readout is a control
            // that cannot be set to anything.
            Line::from(vec![
                Span::styled("Limit: ", label),
                Span::styled(
                    match row.limit_bytes {
                        Some(bytes) => crate::transmission::human_speed(bytes),
                        None => "unlimited".to_string(),
                    },
                    value,
                ),
                Span::styled("   (+ faster, - slower, 0 unlimited)", label),
            ]),
            field("Directory: ", &row.dir, &label, &value),
            field("Added: ", &added(row.added), &label, &value),
        ];

        // The error, when the daemon reported one. It is the only line here
        // that can be a blank space over a working download, and hiding it
        // is how a failed download looks like a stalled one.
        if !row.error.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("Error: {}", row.error),
                Style::default()
                    .fg(self.theme.error_color())
                    .add_modifier(Modifier::BOLD),
            )));
        }

        if !row.trackers.is_empty() {
            lines.push(Line::from(Span::styled("Trackers: ", label)));
            for tracker in &row.trackers {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {} ", tracker.host), value),
                    Span::styled(format!("seeds {}", tracker.seeders), label),
                    Span::styled(format!("  leechers {}", tracker.leechers), label),
                    Span::styled(
                        if tracker.announced {
                            ""
                        } else {
                            "  (no answer)"
                        },
                        label,
                    ),
                ]));
            }
        }

        let _ = config;
        lines
    }

    /// The streaming server's one torrent, for when nothing is being
    /// downloaded. Same shape as above so the two views do not feel like
    /// different programs.
    fn stream_detail_lines(&self, config: &Config) -> Vec<Line<'_>> {
        let s = &self.torrent_status;
        let label = Style::default().fg(self.theme.secondary_color());
        let value = Style::default().fg(self.theme.main_fg.to_color());
        let percent = s.progress * 100.0;
        let state = if self.torrent_paused && !s.hash.is_empty() {
            format!("{} (paused)", s.status)
        } else {
            s.status.clone()
        };
        let history: Vec<f64> = self.progress_history.iter().copied().collect();
        let sparkline = super::widgets::graph::render_sparkline(&history, 40, &config.graph_symbol);
        vec![
            field("Name: ", &s.title, &label, &value),
            field("Hash: ", &s.hash, &label, &value),
            field("State: ", &state, &label, &value),
            Line::from(vec![
                Span::styled("Progress: ", label),
                Span::styled(format!("{percent:.0}%"), value),
                Span::styled("  ", value),
                Span::styled(sparkline, value),
            ]),
            Line::from(vec![
                Span::styled("DL: ", label),
                Span::styled(bytes(s.download_speed), value),
                Span::styled("  ", value),
                Span::styled("UL: ", label),
                Span::styled(bytes(s.upload_speed), value),
            ]),
            field(
                "Downloaded: ",
                &format!("{} / {}", bytes(s.downloaded), bytes(s.total_size)),
                &label,
                &value,
            ),
            field(
                "Ratio: ",
                &s.ratio
                    .map(|r| format!("{r:.2}"))
                    .unwrap_or_else(|| "--".into()),
                &label,
                &value,
            ),
            Line::from(vec![
                Span::styled("Seeds: ", label),
                Span::styled(s.seeds.to_string(), value),
                Span::styled("  Peers: ", label),
                Span::styled(s.peers.to_string(), value),
            ]),
            field("Directory: ", &s.dir, &label, &value),
            field(
                "ETA: ",
                &s.eta
                    .map(|e| format!("{e}s"))
                    .unwrap_or_else(|| "--".into()),
                &label,
                &value,
            ),
        ]
    }

    /// The results table's rows: every filtered row that still has an index to land on.
    fn results_table(&self, seeds: u16, size: u16, date: u16, title_min: u16) -> Option<Table<'_>> {
        // Muted, but not `inactive_fg`: the tab bar gets away with that
        let badge_style = Style::default().fg(self.theme.graph_text.to_color());
        let seed_style = Style::default().fg(self.theme.secondary_color());
        let date_style = Style::default().fg(self.theme.graph_text.to_color());

        let rows: Vec<Row> = self
            .filtered_indices
            .iter()
            .filter_map(|&idx| self.results.get(idx))
            .map(|item| {
                Row::new(vec![
                    Cell::from(item.seeds.as_str()).style(seed_style),
                    Cell::from(item.size.as_str()),
                    Cell::from(item.date.as_str()).style(date_style),
                    Cell::from(source_badge(item)).style(badge_style),
                    Cell::from(item.title.as_str()),
                ])
            })
            .collect();

        // Nothing to tabulate: the caller says why rather than this
        if rows.is_empty() {
            return None;
        }
        Some(
            Table::new(rows, self.results_constraints(seeds, size, date, title_min))
                .header(results_header(&self.theme))
                .row_highlight_style(self.theme.selection_style()),
        )
    }

    /// Why the table has no rows.
    fn render_results_placeholder(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(
            Paragraph::new(self.results_placeholder())
                .style(Style::default().fg(self.theme.graph_text.to_color()))
                .wrap(Wrap { trim: true }),
            area,
        );
    }

    fn results_constraints(
        &self,
        seeds: u16,
        size: u16,
        date: u16,
        title_min: u16,
    ) -> [Constraint; 5] {
        [
            Constraint::Length(seeds),
            Constraint::Length(size),
            Constraint::Length(date),
            Constraint::Length(SOURCE_BADGE_WIDTH),
            Constraint::Min(title_min),
        ]
    }

    /// Which of the table's rows carries the cursor, if any.
    fn results_cursor(&self) -> TableState {
        let mut state = TableState::default();
        if let Some(local_pos) = self
            .filtered_indices
            .iter()
            .position(|&i| i == self.selected)
        {
            state.select(Some(local_pos));
        }
        state
    }

    /// The Results detail view (`R`): the table again, full frame, with
    /// a preview line under it naming the facts of the row under the
    /// cursor -- the detail modal's header row, without the modal.
    fn render_detail_results(&self, frame: &mut Frame, area: Rect, config: &Config) {
        let title = Span::styled(
            " Results detail [R/Esc] close ",
            Style::default().fg(self.theme.primary_color()),
        );
        let block = self
            .themed_block(self.theme.primary_color(), config)
            .title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);

        // The whole terminal is this table, so its fixed columns can be
        let Some(table) = self.results_table(8, 10, 12, 30) else {
            self.render_results_placeholder(frame, chunks[0]);
            return;
        };
        frame.render_stateful_widget(table, chunks[0], &mut self.results_cursor());

        // The preview line: label accents on, facts in the body colour.
        let label = Style::default().fg(self.theme.secondary_color());
        let value = Style::default().fg(self.theme.main_fg.to_color());
        let fact = self
            .filtered_indices
            .iter()
            .find(|&&i| i == self.selected)
            .and_then(|&i| self.results.get(i));
        let preview = match fact {
            None => Line::from(Span::styled("no row selected", label)),
            Some(item) => {
                let hash = if item.info_hash.is_empty() {
                    "-".to_string()
                } else {
                    item.info_hash.clone()
                };
                Line::from(vec![
                    Span::styled("Src: ", label),
                    Span::styled(source_badge(item), value),
                    Span::styled("  Group: ", label),
                    Span::styled(
                        item.group
                            .map_or("-".to_string(), |g| g.label().to_string()),
                        value,
                    ),
                    Span::styled("  Size: ", label),
                    Span::styled(item.size.clone(), value),
                    Span::styled("  Date: ", label),
                    Span::styled(item.date.clone(), value),
                    Span::styled("  Seeds: ", label),
                    Span::styled(item.seeds.clone(), value),
                    Span::styled("  Hash: ", label),
                    Span::styled(hash, value),
                    Span::styled("  Magnet: ", label),
                    Span::styled(
                        if item.magnet.is_some() { "yes" } else { "no" }.to_string(),
                        value,
                    ),
                    Span::styled("  Page: ", label),
                    Span::styled(item.page_url.clone(), value),
                ])
            }
        };
        frame.render_widget(Paragraph::new(preview), chunks[1]);
    }

    fn render_modal(&mut self, frame: &mut Frame, area: Rect, config: &Config) {
        if self.modal == Modal::None {
            return;
        }

        if let Modal::Login(_) = self.modal {
            self.render_login_modal(frame, area, config);
        } else if matches!(self.modal, Modal::Settings(_)) {
            self.render_settings_modal(frame, area, config);
        } else if let Modal::HealthCheck(_) = self.modal {
            self.render_health_modal(frame, area, config);
        } else if matches!(self.modal, Modal::Files(_)) {
            self.render_files_modal(frame, area, config);
        } else if matches!(self.modal, Modal::Help(_)) {
            // `&mut self`: the page publishes its own page count for
            self.render_help_modal(frame, area, config);
        } else if matches!(self.modal, Modal::TorrentDetail(_)) {
            self.render_detail_modal(frame, area, config);
        }
    }
}

/// `Src` sits between the metadata and the title: on the `all` tab a
/// single page mixes trackers, and the row is the only place that says
/// who returned it.
/// Three boxes of two rows each: a top border, the section's two lines,
/// and a bottom border.
const SECTION_HEIGHT: u16 = 4;

/// The Torrents panel's content, split the way its two renderers want it.
struct Downloads<'a> {
    summary: super::torrents_panel::Summary,
    stream: Option<Line<'a>>,
    table: Vec<Line<'a>>,
}

/// One `label: value` line, the shape every detail view uses.
fn field<'a>(label: &'a str, value: &str, label_style: &Style, value_style: &Style) -> Line<'a> {
    Line::from(vec![
        Span::styled(label, *label_style),
        Span::styled(value.to_string(), *value_style),
    ])
}

/// A byte count as a person reads it. Shared with the panel so the two
/// never say `1.9 GB` and `1.9 GB ` about the same number.
fn bytes(n: u64) -> String {
    format_bytes(n)
}

/// A speed, with its unit spelled out.
///
/// The table's `speed()` deliberately drops units so a narrow column fits
/// three of them; here there is room, and a bare `0` reads as a value the
/// daemon failed to report rather than as an idle connection.
fn speed(n: i64) -> String {
    if n <= 0 {
        format_bytes(0)
    } else {
        crate::transmission::human_speed(n)
    }
}

/// When a torrent was added, from the daemon's epoch seconds.
fn added(epoch: i64) -> String {
    if epoch <= 0 {
        return "--".to_string();
    }
    chrono::DateTime::from_timestamp(epoch, 0)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "--".to_string())
}

fn results_header(theme: &Theme) -> Row<'static> {
    Row::new(vec![
        Cell::from("Seeds"),
        Cell::from("Size"),
        Cell::from("Date"),
        Cell::from("Src"),
        Cell::from("Title"),
    ])
    .style(
        Style::default()
            .fg(theme.primary_color())
            .add_modifier(Modifier::BOLD),
    )
}

impl App {
    /// Whether a button's word is drawn bold: a toggle that is
    /// is currently on this way (`Fx::b` around `pause` while
    /// `pause_proc_list`, around `tree` while `proc_tree`,...).
    fn frame_button_active(&self, id: ZoneId, button: &FrameButton) -> bool {
        match (id, button.key) {
            (ZoneId::Results, 'f') => self.zones.filter_mode || !self.zones.filter_input.is_empty(),
            (ZoneId::Torrent, 'p') => self.torrent_paused,
            _ => false,
        }
    }
    /// Draw `id`'s frame legend -- the buttons and the info text, on top
    /// of the border the panel's block has just drawn.
    fn render_frame(&self, frame: &mut Frame, id: ZoneId, area: Rect, config: &Config) {
        let layout = self.frame_layout(id, area, config);
        if layout.info.width > 0 {
            let info = Span::styled(
                layout.info_text,
                Style::default().fg(self.theme.primary_color()),
            );
            frame.render_widget(Paragraph::new(Line::from(info)), layout.info);
        }
        for (button, rect) in &layout.buttons {
            self.draw_frame_button(frame, button, *rect, config, |b| {
                self.frame_button_active(id, b)
            });
        }
    }

    /// One frame button: `┌word key┐` drawn over the border row it sits on.
    ///
    /// Shared with the zone frames so a key written on the panel and the same
    /// key written on the full view are drawn by one piece of code -- the
    /// alternative is two renderings of one convention, and they drift.
    fn draw_frame_button(
        &self,
        frame: &mut Frame,
        button: &super::layout::FrameButton,
        rect: Rect,
        config: &Config,
        active: impl Fn(&super::layout::FrameButton) -> bool,
    ) {
        // The bracket round the word is what makes it read as a
        // control rather than as more of the panel's title -- the
        // reference draws each one as `┌` + letter + word + `┐`
        // -- a bracket on each side, with the plain frame line
        // between them. With "Show boxes" off there is no frame to
        // bracket against, so they go with it.
        let bracketed = config.show_boxes;
        let mut spans = Vec::new();
        if bracketed {
            spans.push(Span::styled(
                "┌",
                Style::default().fg(self.theme.div_line.to_color()),
            ));
        }
        spans.extend(super::layout::button_spans(
            &self.theme,
            button,
            active(button),
            self.hovers(rect),
        ));
        if bracketed {
            spans.push(Span::styled(
                "┐",
                Style::default().fg(self.theme.div_line.to_color()),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), rect);
    }

    /// Keys along the **bottom border of a box**, left aligned, dropping
    /// whole buttons that do not fit.
    ///
    /// The keys belong to the downloads box -- the box whose contents they
    /// act on -- so they are written on *its* frame, not on the outer one
    /// that holds the whole view. A keybind row on the view's own frame
    /// says "this is what this view is"; on the box it says "this is what
    /// these rows do", which is the difference between a caption and a set
    /// of controls. And a `┌unlim…` cut off by the frame is a key that
    /// reads as a typo.
    fn render_buttons_on_bottom_border(
        &self,
        frame: &mut Frame,
        box_area: Rect,
        buttons: &[super::layout::FrameButton],
        config: &Config,
    ) {
        if box_area.height < 2 || box_area.width < 2 {
            return; // no border row to write on
        }
        let bottom = box_area.y + box_area.height - 1;
        let mut x = box_area.x;
        for button in buttons {
            if x + button.width() > box_area.x + box_area.width {
                break;
            }
            self.draw_frame_button(
                frame,
                button,
                Rect::new(x, bottom, button.width(), 1),
                config,
                |_| false,
            );
            x += button.width() + super::view::FRAME_GAP;
        }
    }
}

/// How much of a source's error text a Sources row shows.
const STATUS_TEXT_WIDTH: usize = 24;

/// What a Sources row appends after its checkbox: what that source answered for the search that
/// ran.
fn source_status_text(status: &SourceStatus) -> String {
    match status {
        SourceStatus::Pending => "…".to_string(),
        SourceStatus::Ok(rows) => format!("✓ {rows}"),
        SourceStatus::Timeout => "✗ timeout".to_string(),
        SourceStatus::Error(err) => {
            let clipped: String = err.chars().take(STATUS_TEXT_WIDTH).collect();
            if err.chars().count() > STATUS_TEXT_WIDTH {
                format!("✗ {clipped}…")
            } else {
                format!("✗ {err}")
            }
        }
    }
}

/// The status's own colour, so a failure reads at a glance: dim while still in flight, the
/// informational mid-bright for an answer, the error accent for a refusal.
pub(super) fn source_status_style(status: &SourceStatus, theme: &Theme) -> Style {
    match status {
        SourceStatus::Pending => Style::default().fg(theme.inactive_fg.to_color()),
        SourceStatus::Ok(_) => Style::default().fg(theme.graph_text.to_color()),
        SourceStatus::Timeout | SourceStatus::Error(_) => Style::default().fg(theme.error_color()),
    }
}

/// The detail log's line colour: the error accent for a refusal, `secondary` for a success,
/// `primary` for a warning, the body colour for everything else.
pub(super) fn detail_log_style(line: &str, theme: &Theme) -> Style {
    if line.contains("ERROR") || line.contains("FAIL") || line.contains("error:") {
        Style::default().fg(theme.error_color())
    } else if line.contains("OK") || line.contains("SUCCESS") || line.contains("logged in") {
        Style::default().fg(theme.secondary_color())
    } else if line.contains("WARN") {
        Style::default().fg(theme.primary_color())
    } else {
        Style::default().fg(theme.main_fg.to_color())
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
