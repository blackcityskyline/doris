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
    /// The block every zone frame is built from: themed, bordered, and
    /// titled with the zone's own number and label.
    ///
    /// Four renderers want exactly this, and each was spelling out the
    /// same border-colour-then-title pair. What it costs to write it out
    /// is that the label is where the focus mark lives, so a change to it
    /// has to find all four or one zone stops showing it has focus.
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

    /// Border+background styling for the four main zone panels,
    /// respecting the "Rounded corners", "Theme background" and "Show
    /// boxes" Options toggles. Centralizes what used to be ~14 separate
    /// hand-rolled `Block::default()...` call sites, each of which would
    /// have needed this same check repeated -- previously these settings
    /// were persisted in Config but had no rendering effect anywhere.
    fn themed_block(&self, border_color: Color, config: &Config) -> Block<'static> {
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
        // hides the content underneath and the terminal's own background
        // shows through.
        if config.theme_background {
            block = block.style(
                Style::default().bg(self.resolve_color(self.theme.main_bg.to_color(), config)),
            );
        }
        block
    }

    /// Degrade an RGB color per the "Truecolor"/"False tty" toggles; see
    /// `theme::degrade_color`. Named/basic colors pass through untouched.
    fn resolve_color(&self, color: Color, config: &Config) -> Color {
        if config.false_tty {
            super::theme::degrade_color(color, false)
        } else if !config.truecolor {
            super::theme::degrade_color(color, true)
        } else {
            color
        }
    }

    /// Draw the main view. `config` rides along because the zones read
    /// it: the Trackers panel's checkboxes and the Results frame's "what
    /// the search is asking" slot both come from `enabled_sources`.
    pub fn render(&mut self, frame: &mut Frame, config: &Config) {
        let area = frame.area();

        if self.show_menu {
            self.render_menu_view(frame, area, config);
            return;
        }

        self.render_main_view(frame, area, config);
    }

    fn render_menu_view(&mut self, frame: &mut Frame, area: Rect, config: &Config) {
        self.zones.update_areas(area);
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
                ZoneId::Trackers => self.render_trackers_zone(frame, zone_area, *zone_id, config),
            }
        }
        let backdrop = self
            .modal_block(self.theme.primary_color(), config)
            .title(Span::styled(
                " menu ",
                Style::default().fg(self.theme.primary_color()),
            ));
        super::menu::render_menu(frame, area, &self.menu, &self.theme, backdrop);
    }

    fn render_main_view(&mut self, frame: &mut Frame, area: Rect, config: &Config) {
        self.zones.update_areas(area);

        // A zone with no detail view (`Trackers`) simply has nothing to
        // take over with, so it falls through to the tiled render rather
        // than panicking on a state no key can produce.
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
        // the help page (`?`) and the frame legends now, and what this
        // box needs to say is what it is holding. The only thing that
        // changes is an active filter; while the box has the keyboard it
        // takes the accent a focused zone frame takes, and otherwise the
        // divider colour -- the same "colour says state, text says
        // content" split btop's boxes use.
        let filter_on = self.zones.filter_mode || !self.zones.filter_input.is_empty();
        // While `f` is open the text lives in the box, so the border
        // names the mode only -- repeating it there was the third copy
        // (border, body, frame legend). A filter in effect but not
        // being edited still announces itself: the box then holds the
        // query, and nothing else says a filter is on.
        let title = match (self.input_mode, self.zones.filter_mode, filter_on) {
            (false, true, _) => "filter".to_string(),
            (false, false, true) => format!("filter: {}", self.zones.filter_input),
            _ => "Search".to_string(),
        };

        // The box holds whichever string is being edited: the query in
        // search mode, the filter while `f` is open. Before, the filter
        // only ever reached the border title, so the box kept showing
        // the stale query while the text grew somewhere else.
        let editing: &str = if self.zones.filter_mode {
            self.zones.filter_input.as_str()
        } else {
            self.search_input.as_str()
        };

        let input_border = self
            .themed_block(
                if self.input_mode || self.zones.filter_mode {
                    // The box under the cursor takes the same accent a
                    // focused zone frame takes; the title says which of
                    // the two boxes it is.
                    self.theme.primary_color()
                } else {
                    self.theme.div_line.to_color()
                },
                config,
            )
            .title(Span::styled(
                title,
                Style::default().fg(self.theme.primary_color()),
            ));

        let inner = input_border.inner(bar_area);
        let input = Paragraph::new(editing)
            .block(input_border)
            .style(Style::default().fg(self.theme.main_fg.to_color()));

        frame.render_widget(input, bar_area);

        // A text field without a caret is a text field you type into
        // blind. The terminal hides it again on the next frame that
        // does not ask for one, so an idle box stays clean.
        if (self.input_mode || self.zones.filter_mode)
            && self.modal == Modal::None
            && !self.show_menu
        {
            let col = editing.chars().count() as u16;
            frame.set_cursor_position((inner.x + col.min(inner.width.saturating_sub(1)), inner.y));
        }
    }

    fn render_results_zone(&self, frame: &mut Frame, area: Rect, id: ZoneId, config: &Config) {
        let block = self.zone_block(id, config);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // One row inside the border: the table. The category row that
        // used to sit above it moved onto the frame (П.4 follow-up) --
        // the current category is read off the `◀ name ▶` button next to
        // `group`, so a row here would have been a second copy of it.
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0)]) // results table
            .split(inner);

        // --- results table ---------------------------------------------------
        // Widths are the panel's, not the detail view's: this one shares
        // the frame with the zone buttons.
        let Some(table) = self.results_table(6, 8, 8, 20) else {
            // The block is already on screen, so this only fills its
            // inner area.
            self.render_results_placeholder(frame, inner);
            self.render_frame(frame, id, area, config);
            return;
        };
        frame.render_stateful_widget(table, chunks[0], &mut self.results_cursor());

        // The keybind legend moved onto the frame with П.5, so the panel
        // body ends at the table and every remaining line is data.
        self.render_frame(frame, id, area, config);
    }

    /// The Trackers panel: the `all` master switch on top, then one
    /// row per registered source, `[x]`/`[ ]` showing whether the search
    /// asks it. The row under the cursor is reversed, the same way the
    /// selected result row is -- the cursor is the panel's only state, and
    /// it has to be visible the same way.
    fn render_trackers_zone(&self, frame: &mut Frame, area: Rect, id: ZoneId, config: &Config) {
        let block = self.zone_block(id, config);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // The status reads as a column only if every id is padded to
        // the widest one, so an answer lands under the answer above it
        // instead of trailing each name at its own length.
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
                let mut spans = vec![Span::styled(
                    format!("[{}] {:<width$}", mark, row.id(), width = id_width),
                    style,
                )];
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
                // visible where it will land -- but says so instead of
                // pretending it can be switched on.
                if !row.is_implemented() {
                    spans.push(Span::styled(" (planned)", style));
                }
                Line::from(spans)
            })
            .collect();

        // The panel can be hidden (`1`-`4` again) and the terminal can
        // be too short for the roster, so the window follows the cursor -- the
        // rule the detail modal's file list already uses. Without it the
        // rows past the end were simply not drawn while `j`/`k` kept
        // walking every one of them, and the cursor could stand on a row
        // nobody could see.
        let visible = inner.height as usize;
        let offset = self
            .sources_cursor
            .saturating_sub(visible.saturating_sub(1));
        let shown: Vec<Line> = rows.into_iter().skip(offset).take(visible).collect();
        frame.render_widget(Paragraph::new(shown), inner);

        self.render_frame(frame, id, area, config);
    }

    fn render_torrent_zone(&self, frame: &mut Frame, area: Rect, id: ZoneId, config: &Config) {
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
        // hash arriving there is real work in flight (session, magnet,
        // add, upload) and no value to print, and an empty
        // `Hash: Status:` read as "nothing is happening".
        // Labels take the secondary accent, values the body colour: the
        // panel is data, so only the words that name a value are
        // accented. A finished transfer gets the primary accent rather
        // than the hardcoded green the theme never knew about.
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
        // the facts stay readable, and the question is the only thing in
        // the panel drawn in the error accent, which is what that accent
        // is for.
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
        // are frame buttons now, top-right and bottom-left.
        self.render_frame(frame, id, area, config);
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
        // frame, next to the `detail` button.
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

    /// The Torrent detail view (`T`): the same facts the panel draws in
    /// four tight lines, given the whole frame -- so the name gets a
    /// line of its own and the bar is as wide as the terminal instead
    /// of the panel's 50-column cap.
    fn render_detail_torrent(&self, frame: &mut Frame, area: Rect, config: &Config) {
        let s = &self.torrent_status;
        let progress_pct = (s.progress * 100.0) as u32;
        // `Progress: NN% ` leads, so the sparkline gets what is left
        // rather than eating the number off the end of a too-wide line.
        let prefix = format!("Progress: {}% ", progress_pct).len() as u16;
        let bar_width = area.width.saturating_sub(prefix + 2) as usize;

        let history: Vec<f64> = self.progress_history.iter().copied().collect();
        let sparkline =
            super::widgets::graph::render_sparkline(&history, bar_width, &config.graph_symbol);

        let status_display = if self.torrent_paused && !s.hash.is_empty() {
            format!("{} (paused)", s.status)
        } else {
            s.status.clone()
        };

        // Same split as the panel: labels in the secondary accent, the
        // values they name in the body colour.
        let label = Style::default().fg(self.theme.secondary_color());
        let value = Style::default().fg(self.theme.main_fg.to_color());

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Name: ", label),
                Span::styled(s.title.clone(), value),
            ]),
            Line::from(vec![
                Span::styled("Hash: ", label),
                Span::styled(s.hash.clone(), value),
            ]),
            Line::from(vec![
                Span::styled("Status: ", label),
                Span::styled(status_display, value),
            ]),
        ];
        lines.push(Line::from(vec![
            Span::styled("Progress: ", label),
            Span::styled(
                format!("{}%", progress_pct),
                if progress_pct >= 100 {
                    Style::default().fg(self.theme.primary_color())
                } else {
                    value
                },
            ),
            Span::styled(" ", value),
            Span::styled(sparkline, value),
        ]));
        lines.push(Line::from(vec![
            Span::styled("DL: ", label),
            Span::styled(format_bytes(s.download_speed), value),
            Span::styled("  ", value),
            Span::styled("UL: ", label),
            Span::styled(format_bytes(s.upload_speed), value),
            Span::styled("  Downloaded: ", label),
            Span::styled(format_bytes(s.downloaded), value),
            Span::styled(" / ", value),
            Span::styled(format_bytes(s.total_size), value),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Seeds: ", label),
            Span::styled(s.seeds.to_string(), value),
            Span::styled("  Peers: ", label),
            Span::styled(s.peers.to_string(), value),
        ]));

        let title = Span::styled(
            " Torrent detail [T/Esc] close ",
            Style::default().fg(self.theme.primary_color()),
        );
        let paragraph = Paragraph::new(lines).block(
            self.themed_block(self.theme.primary_color(), config)
                .title(title),
        );
        frame.render_widget(paragraph, area);
    }

    /// The results table's rows: every filtered row that still has an
    /// index to land on.
    ///
    /// The panel and its detail view draw the same table, so this is the
    /// one place the row is built. What they do differ on is the column
    /// widths -- the detail view has the whole terminal, so its fixed
    /// columns can be wider -- which is why the widths are a parameter
    /// and not a constant here.
    fn results_table(&self, seeds: u16, size: u16, date: u16, title_min: u16) -> Option<Table<'_>> {
        // Muted, but not `inactive_fg`: the tab bar gets away with that
        // one because a tab is also spelled out in the title. Here the
        // badge is the only thing saying who returned the row, and on
        // the `all` tab that is the point of the column -- so it takes
        // the informational mid-bright `graph_text` instead (≈6.7:1 on
        // `main_bg`, versus ≈2.3:1 for `inactive_fg`), which the cursor
        // row then writes in the theme's `selected_fg`.
        //
        // Accents, not a repaint: a row keeps the body colour and only two
        // columns carry a token -- the seed count in the secondary accent
        // (seed health), the date in the informational mid-bright next to
        // the source badge. Size and title stay plain.
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
        // drawing the header over an empty body, which read as a broken
        // table.
        if rows.is_empty() {
            return None;
        }
        Some(
            Table::new(rows, self.results_constraints(seeds, size, date, title_min))
                .header(results_header(&self.theme))
                .row_highlight_style(self.theme.selection_style()),
        )
    }

    /// Why the table has no rows. Drawn instead of the header so an
    /// empty Results panel reads as "nothing yet" rather than as a
    /// broken table.
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
        // wider than the panel's.
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
        } else if matches!(self.modal, Modal::Help(_)) {
            // `&mut self`: the page publishes its own page count for
            // `help_key` while it draws.
            self.render_help_modal(frame, area, config);
        } else if matches!(self.modal, Modal::TorrentDetail(_)) {
            self.render_detail_modal(frame, area, config);
        }
    }
}

/// `Src` sits between the metadata and the title: on the `all` tab a
/// single page mixes trackers, and the row is the only place that says
/// who returned it.
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
    /// Whether a button's word is drawn bold: btop marks a toggle that
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
            // Hovered: whole-cell containment against the same rectangle
            // `click_at` hits, so what is drawn and what is clickable are
            // the same rectangle rather than two computations of it.
            let hovered = self.hovers(*rect);
            let spans = super::layout::button_spans(
                &self.theme,
                button,
                self.frame_button_active(id, button),
                hovered,
            );
            frame.render_widget(Paragraph::new(Line::from(spans)), *rect);
        }
    }
}

/// How much of a source's error text a Sources row shows. An error is a
/// sentence, and a sentence does not fit on a one-line row.
const STATUS_TEXT_WIDTH: usize = 24;

/// What a Sources row appends after its checkbox: what that source
/// answered for the search that ran. Without it the panel said only
/// which sources were *asked*, never which of them replied.
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

/// The status's own colour, so a failure reads at a glance: dim while
/// still in flight, the informational mid-bright for an answer, the
/// error accent for a refusal. The label says it in words too, so
/// colour is never the only channel carrying the meaning.
pub(super) fn source_status_style(status: &SourceStatus, theme: &Theme) -> Style {
    match status {
        SourceStatus::Pending => Style::default().fg(theme.inactive_fg.to_color()),
        SourceStatus::Ok(_) => Style::default().fg(theme.graph_text.to_color()),
        SourceStatus::Timeout | SourceStatus::Error(_) => Style::default().fg(theme.error_color()),
    }
}

/// The detail log's line colour: the error accent for a refusal,
/// `secondary` for a success, `primary` for a warning, the body colour
/// for everything else. Severity is spelled in the line as well
/// (`ERROR`/`OK`/`WARN`), so the colour is a second channel, never the
/// only one -- and never a colour the theme did not choose: these were
/// hardcoded red/green/yellow before the tokens existed.
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
