//! Pulling the border between the `T` view's two boxes.
//!
//! The Torrents full-frame view is two boxes under three summary boxes: the
//! downloads table and the facts of the row under its cursor. Both want more
//! rows than most terminals show, so the view has to divide what it has --
//! and a division the user cannot change is a division they have to live
//! with. This is the zone border drag applied to one divider: the pointer
//! grabs the border row between the boxes and moves it, and
//! `ctrl+shift+up`/`ctrl+shift+down` move it a row at a time, which is the
//! same chord the zones use for their own dividers.
//!
//! Nothing else in the view is draggable. The summary boxes are four rows
//! tall and fixed: there is no second arrangement of three boxes that is not
//! worse, and a divider that does nothing when pulled is worse than no
//! divider at all.
//!
//! The heights come from [`crate::ui::torrents_panel::detail_budget`] through
//! `App::detail_budget_at`, the same call the renderer makes, so the row that
//! can be grabbed is the row that is drawn.

use ratatui::layout::Rect;

use crate::config::Config;
use crate::ui::layout::ZoneId;
use crate::ui::view::App;

impl App {
    /// The row of the border between the two boxes -- the one row that can be
    /// grabbed. `None` when there is nothing to trade rows with.
    pub fn detail_divider_row(&self, area: Rect, config: &Config) -> Option<u16> {
        if self.detail_view != Some(ZoneId::Torrent) {
            return None;
        }
        let budget = self.detail_budget_at(area, config);
        // No facts box means no divider: the table is the whole view and
        // there is nothing below it to share the rows with.
        if budget.facts_height < 3 {
            return None;
        }
        let inner = self.detail_inner(area);
        Some(inner.y + budget.sections_height + budget.downloads_height - 1)
    }

    /// The pointer went down on the divider.
    pub fn detail_resize_start(&mut self, row: u16, area: Rect, config: &Config) -> bool {
        match self.detail_divider_row(area, config) {
            Some(divider) if divider == row => {
                self.detail_dragging = true;
                true
            }
            _ => false,
        }
    }

    /// The pointer is on the divider and has moved: the table takes the rows
    /// between where the summary boxes end and where the pointer is.
    pub fn detail_resize_drag(&mut self, row: u16, area: Rect, config: &Config) {
        if !self.detail_dragging {
            return;
        }
        let budget = self.detail_budget_at(area, config);
        let top = self.detail_inner(area).y + budget.sections_height;
        self.detail_split = Some(row.saturating_sub(top).saturating_add(1));
    }

    pub fn detail_resize_end(&mut self) {
        self.detail_dragging = false;
    }

    /// One row of the divider, from the keyboard: the same chord the zones
    /// use, so "what moves a divider" is one thing in doris and not two.
    pub fn detail_resize_key(&mut self, up: bool, area: Rect, config: &Config) {
        if self.detail_view != Some(ZoneId::Torrent) {
            return;
        }
        let budget = self.detail_budget_at(area, config);
        if budget.facts_height < 3 {
            return;
        }
        let current = self.detail_split.unwrap_or(budget.downloads_height);
        let next = if up {
            current.saturating_sub(1)
        } else {
            current.saturating_add(1)
        };
        self.detail_split = Some(next);
    }
}
