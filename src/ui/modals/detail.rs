//! The torrent detail modal: everything the selected row knows about itself, plus the file list
//! its source can read off the row's page. Extracted from `ui/app.rs` with the other modals;
//! `render_modal` dispatches to `render_detail_modal`.

use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::config::Config;
use crate::sources::source::Group;
use crate::ui::view::{centered_rect, App, Modal};

/// What a fact the row does not carry reads as. The modal already spells `-`
/// for a missing hash and a missing magnet; these two are the same question.
fn fallback(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else {
        value
    }
}

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
                Span::styled("By:      ", label),
                Span::styled(fallback(&state.item.uploader), value),
            ]),
            Line::from(vec![
                Span::styled("Where:   ", label),
                Span::styled(fallback(&state.item.category), value),
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

        // No background of its own: `Clear` has blanked the popup and
        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }
}
