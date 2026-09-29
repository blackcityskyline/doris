//! The torrent detail modal (UI_REFACTOR_PLAN §7): everything the
//! selected row knows about itself, plus the file list its source can
//! read off the row's page. Extracted from `ui/app.rs` with the other
//! modals; `render_modal` dispatches to [`render_detail_modal`].
//!
//! The row's own facts are drawn before anything is fetched, so the
//! modal is never an empty box: the file list arrives as
//! `Event::DetailLoaded` and lands in the state the render pass reads.

use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::config::Config;
use crate::sources::source::Group;
use crate::ui::app::{centered_rect, App, Modal};

impl App {
    /// Draw the detail modal: `Label: value` rows for the row itself,
    /// then the file list with the cursor reversed, then the keys.
    pub fn render_detail_modal(&self, frame: &mut Frame, area: Rect, config: &Config) {
        let Modal::TorrentDetail(ref state) = self.modal else {
            return;
        };

        let popup = centered_rect(70, 80, area);
        frame.render_widget(Clear, popup);

        let block = self
            .modal_block(self.theme.primary_color(), config)
            .title(Span::styled(
                " Torrent details ",
                Style::default().fg(self.theme.primary_color()),
            ));

        let inner = block.inner(popup);
        frame.render_widget(block, popup);

        let label = Style::default()
            .fg(self.theme.secondary_color())
            .add_modifier(Modifier::BOLD);
        let value = Style::default().fg(self.theme.main_fg.to_color());
        let mut lines: Vec<Line> = vec![
            Line::from(vec![
                Span::styled("Title:   ", label),
                Span::styled(state.item.title.as_str(), value),
            ]),
            Line::from(vec![
                Span::styled("Source:  ", label),
                Span::styled(
                    if state.item.source.is_empty() {
                        "-"
                    } else {
                        state.item.source.as_str()
                    },
                    value,
                ),
            ]),
            Line::from(vec![
                Span::styled("Size:    ", label),
                Span::styled(state.item.size.as_str(), value),
            ]),
            Line::from(vec![
                Span::styled("Seeds:   ", label),
                Span::styled(state.item.seeds.as_str(), value),
            ]),
            Line::from(vec![
                Span::styled("Date:    ", label),
                Span::styled(state.item.date.as_str(), value),
            ]),
            Line::from(vec![
                Span::styled("Group:   ", label),
                Span::styled(state.item.group.map_or("-", Group::label), value),
            ]),
            Line::from(vec![
                Span::styled("Hash:    ", label),
                Span::styled(
                    if state.item.info_hash.is_empty() {
                        "-"
                    } else {
                        state.item.info_hash.as_str()
                    },
                    value,
                ),
            ]),
            Line::from(vec![
                Span::styled("Magnet:  ", label),
                Span::styled(state.item.magnet.as_deref().unwrap_or("-"), value),
            ]),
            Line::from(vec![
                Span::styled("Page:    ", label),
                Span::styled(
                    if state.item.page_url.is_empty() {
                        "-"
                    } else {
                        state.item.page_url.as_str()
                    },
                    value,
                ),
            ]),
        ];

        // The file list: what the source could read off the row's page,
        // or an honest line saying it could not.
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("Files:", label)));
        if state.pending {
            lines.push(Line::from(Span::styled(
                "  reading the torrent's page...",
                Style::default().fg(self.theme.inactive_fg.to_color()),
            )));
        } else if let Some(ref err) = state.error {
            lines.push(Line::from(Span::styled(
                format!("  could not read the file list: {}", err),
                Style::default().fg(self.theme.inactive_fg.to_color()),
            )));
        } else if state.files.is_empty() {
            lines.push(Line::from(Span::styled(
                "  this source cannot list the files",
                Style::default().fg(self.theme.inactive_fg.to_color()),
            )));
        } else {
            // The list can be longer than the box, so the window follows
            // the cursor -- the same rule the results table's selection
            // uses, and the reason the cursor exists at all.
            let visible = inner.height as usize;
            let offset = state.cursor.saturating_sub(visible.saturating_sub(1));
            for (i, file) in state.files.iter().enumerate().skip(offset).take(visible) {
                let text = format!("  {}  ({})", file.name, file.size);
                let style = if i == state.cursor {
                    self.theme.selection_style()
                } else {
                    value
                };
                lines.push(Line::from(Span::styled(text, style)));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Enter: play  d: download  Esc: close",
            Style::default().fg(self.theme.inactive_fg.to_color()),
        )));

        let paragraph = Paragraph::new(lines).style(Style::default().bg(Color::DarkGray));
        frame.render_widget(paragraph, inner);
    }
}
