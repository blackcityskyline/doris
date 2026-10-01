use super::theme::Theme;
use ratatui::prelude::*;

/// Rows the always-visible search input takes: top border, one line of text, bottom border.
pub const SEARCH_BAR_HEIGHT: u16 = 3;

/// How small a mouse drag may leave a zone: still a frame, still a border to grab again, still
/// room for the one line a panel needs to say anything.
pub const RESIZE_MIN_HEIGHT: u16 = 3;

/// The width floor, wider than the height one because a table that
/// cannot show its columns is not a panel -- it is a scrollbar with
/// ambitions.
pub const RESIZE_MIN_WIDTH: u16 = 10;

/// A border the pointer is holding, or the one a click just armed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeKind {
    /// The horizontal divider between the row holding `above` and the
    /// row holding `below`.
    Row { above: ZoneId, below: ZoneId },
    /// The vertical divider between the cell holding `left` and the
    /// one holding `right` -- the two are in the same row.
    Col { left: ZoneId, right: ZoneId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneId {
    Results = 1,
    Torrent = 2,
    /// The trackers checklist: which sources the search asks.
    Trackers = 3,
    /// The log moved to the last slot so the zone keyboard reads
    /// Results / Torrent / Trackers / Log in row order.
    Log = 4,
}

/// `(digit key, label)` per zone.
const ZONE_ROWS: &[(ZoneId, char, &str)] = &[
    (ZoneId::Results, '1', "Results"),
    (ZoneId::Torrent, '2', "Torrent"),
    (ZoneId::Trackers, '3', "Trackers"),
    (ZoneId::Log, '4', "Log"),
];

impl ZoneId {
    pub fn all() -> &'static [ZoneId] {
        &[
            ZoneId::Results,
            ZoneId::Torrent,
            ZoneId::Trackers,
            ZoneId::Log,
        ]
    }

    pub fn key_char(&self) -> char {
        ZONE_ROWS
            .iter()
            .find(|(id, ..)| id == self)
            .map_or('\0', |&(_, c, _)| c)
    }

    pub fn label(&self) -> &'static str {
        ZONE_ROWS
            .iter()
            .find(|(id, ..)| id == self)
            .map_or("", |&(_, _, label)| label)
    }

    pub fn from_key(c: char) -> Option<ZoneId> {
        ZONE_ROWS
            .iter()
            .find(|&(_, key, _)| *key == c)
            .map(|&(id, ..)| id)
    }

    /// The key that takes over the frame with this zone's detail view (`L`/`T`/`R`), or `None`
    /// for a zone that has none.
    pub fn detail_key(&self) -> Option<char> {
        match self {
            ZoneId::Results => Some('R'),
            ZoneId::Torrent => Some('T'),
            ZoneId::Trackers => None,
            ZoneId::Log => Some('L'),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Zone {
    pub id: ZoneId,
    pub visible: bool,
    pub area: Rect,
    /// Its share of its row's width: `1.0` is an equal split, which is
    /// exactly what the layout did before there was a share at all.
    pub flex: f32,
    /// Its share of the height among rows.
    pub row_flex: f32,
}

impl Zone {
    pub fn new(id: ZoneId, visible: bool) -> Self {
        Self {
            id,
            visible,
            area: Rect::default(),
            flex: 1.0,
            row_flex: 1.0,
        }
    }

    fn reset_flex(&mut self) {
        self.flex = 1.0;
        self.row_flex = 1.0;
    }
}

pub struct ZoneLayout {
    pub zones: Vec<Zone>,
    pub focused: ZoneId,
    pub fullscreen: Option<ZoneId>,
    pub filter_mode: bool,
    pub filter_input: String,
    /// The tiling: rows of cells, one cell per zone, left to right and top to bottom.
    pub grid: Vec<Vec<ZoneId>>,
    /// The border the pointer is holding, set by [`Self::resize_start`] on a click and cleared
    /// by [`Self::resize_end`] on the release.
    pub resize: Option<ResizeKind>,
}

impl Default for ZoneLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl ZoneLayout {
    pub fn new() -> Self {
        Self {
            zones: vec![
                Zone::new(ZoneId::Results, true),
                Zone::new(ZoneId::Torrent, true),
                Zone::new(ZoneId::Trackers, true),
                Zone::new(ZoneId::Log, true),
            ],
            grid: ZoneId::all().iter().map(|&id| vec![id]).collect(),
            focused: ZoneId::Results,
            fullscreen: None,
            filter_mode: false,
            filter_input: String::new(),
            resize: None,
        }
    }

    pub fn toggle(&mut self, id: ZoneId) {
        if self.fullscreen == Some(id) {
            self.fullscreen = None;
            return;
        }
        if let Some(zone) = self.zones.iter_mut().find(|z| z.id == id) {
            zone.visible = !zone.visible;
            if zone.visible && self.fullscreen.is_none() {
                self.focused = id;
            }
        }
    }

    /// What a zone digit means.
    pub fn focus_or_toggle(&mut self, id: ZoneId) {
        if self.fullscreen == Some(id) {
            self.fullscreen = None;
            return;
        }
        let state = self
            .zones
            .iter()
            .find(|z| z.id == id)
            .map(|z| (z.visible, self.focused == id));
        match state {
            None => {}
            Some((false, _)) => {
                self.set_visible(id, true);
                if self.fullscreen.is_none() {
                    self.focused = id;
                }
            }
            Some((true, false)) => {
                if self.fullscreen.is_none() {
                    self.focused = id;
                }
            }
            Some((true, true)) => {
                self.set_visible(id, false);
                self.focus_after_hiding(id);
            }
        }
    }

    /// Move the focus to the first zone that is still visible *after*
    /// `hidden` in zone order, wrapping around -- `focus_next` starts
    /// from the cursor's own position, and the cursor's own position is
    /// the zone that has just disappeared.
    fn focus_after_hiding(&mut self, hidden: ZoneId) {
        let all = ZoneId::all();
        let start = all.iter().position(|z| *z == hidden).unwrap_or(0);
        let next = (1..=all.len())
            .map(|step| all[(start + step) % all.len()])
            .find(|&id| id != hidden && self.is_visible(id));
        if let Some(id) = next {
            self.focused = id;
        }
    }

    /// Set a zone's visibility directly, rather than flipping it.
    pub fn set_visible(&mut self, id: ZoneId, visible: bool) {
        if let Some(zone) = self.zones.iter_mut().find(|z| z.id == id) {
            zone.visible = visible;
        }
        if !visible && self.fullscreen == Some(id) {
            self.fullscreen = None;
        }
    }

    pub fn apply_preset(&mut self, spec: &str) {
        self.grid = Self::parse_spec(spec);
        // A preset is a *new* arrangement, and the weights are a
        // property of the old one: a Results row made tall for the
        // four-row tiling would otherwise swallow the two-row one.
        for zone in &mut self.zones {
            zone.reset_flex();
        }
        self.resize = None;
        let wanted: std::collections::HashSet<char> =
            spec.chars().filter(|c| c.is_ascii_digit()).collect();
        for id in ZoneId::all() {
            self.set_visible(*id, wanted.contains(&id.key_char()));
        }
        if !self.is_visible(self.focused) {
            if let Some(first_visible) = self.zones.iter().find(|z| z.visible).map(|z| z.id) {
                self.focused = first_visible;
            }
        }
    }

    pub fn focus_next(&mut self) {
        let visible: Vec<ZoneId> = self
            .zones
            .iter()
            .filter(|z| z.visible)
            .map(|z| z.id)
            .collect();
        if visible.is_empty() {
            return;
        }
        if let Some(pos) = visible.iter().position(|&z| z == self.focused) {
            let next = (pos + 1) % visible.len();
            self.focused = visible[next];
        } else {
            self.focused = visible[0];
        }
    }

    pub fn focus_prev(&mut self) {
        let visible: Vec<ZoneId> = self
            .zones
            .iter()
            .filter(|z| z.visible)
            .map(|z| z.id)
            .collect();
        if visible.is_empty() {
            return;
        }
        if let Some(pos) = visible.iter().position(|&z| z == self.focused) {
            let prev = if pos == 0 { visible.len() - 1 } else { pos - 1 };
            self.focused = visible[prev];
        } else {
            self.focused = visible[0];
        }
    }

    pub fn set_fullscreen(&mut self, id: Option<ZoneId>) {
        self.fullscreen = id;
    }

    pub fn update_areas(&mut self, area: Rect) {
        if let Some(fs_id) = self.fullscreen {
            for zone in &mut self.zones {
                zone.area = if zone.id == fs_id {
                    area
                } else {
                    Rect::default()
                };
            }
            return;
        }

        self.layout_grid(area);
    }

    /// Split a spec into rows of zone keys: `,` starts a new row, and the digits inside one
    /// chunk are its cells left to right.
    fn parse_spec(spec: &str) -> Vec<Vec<ZoneId>> {
        spec.split(',')
            .map(|row| row.chars().filter_map(ZoneId::from_key).collect())
            .filter(|row: &Vec<ZoneId>| !row.is_empty())
            .collect()
    }

    /// The grid: rows from `grid` top to bottom, cells left to right, each row taking an equal
    /// share of the height below the search bar and each cell an equal share of its row's width
    /// -- the first row and the first cells of a row taking the remainder, so no row of the
    /// terminal is wasted.
    fn layout_grid(&mut self, area: Rect) {
        let rows = self.visible_rows();

        if rows.is_empty() {
            for zone in &mut self.zones {
                zone.area = Rect::default();
            }
            return;
        }

        let available = area.height.saturating_sub(SEARCH_BAR_HEIGHT);
        let heights = distribute(
            available,
            &rows
                .iter()
                .map(|row| self.row_weight(row))
                .collect::<Vec<_>>(),
        );

        let mut y = area.y + SEARCH_BAR_HEIGHT;
        for (row, &h) in rows.iter().zip(heights.iter()) {
            let weights = row.iter().map(|id| self.flex_of(*id)).collect::<Vec<_>>();
            let widths = distribute(area.width, &weights);
            let mut x = area.x;
            for (&id, &w) in row.iter().zip(widths.iter()) {
                self.set_area(id, Rect::new(x, y, w, h));
                x += w;
            }
            y += h;
        }

        for zone in &mut self.zones {
            if !zone.visible {
                zone.area = Rect::default();
            }
        }
    }

    /// The rows to lay out, in drawing order: the grid's own rows with hidden zones dropped,
    /// then any visible zone the spec never named as a row of its own at the bottom.
    fn visible_rows(&self) -> Vec<Vec<ZoneId>> {
        let mut rows: Vec<Vec<ZoneId>> = self
            .grid
            .iter()
            .map(|row| {
                row.iter()
                    .copied()
                    .filter(|id| self.is_visible(*id))
                    .collect()
            })
            .filter(|row: &Vec<ZoneId>| !row.is_empty())
            .collect();
        for zone in &self.zones {
            if zone.visible && !self.grid.iter().flatten().any(|id| *id == zone.id) {
                rows.push(vec![zone.id]);
            }
        }
        rows
    }

    fn flex_of(&self, id: ZoneId) -> f32 {
        self.zone(id).map_or(1.0, |z| z.flex)
    }

    fn row_flex_of(&self, id: ZoneId) -> f32 {
        self.zone(id).map_or(1.0, |z| z.row_flex)
    }

    fn zone(&self, id: ZoneId) -> Option<&Zone> {
        self.zones.iter().find(|z| z.id == id)
    }

    /// A row's height share, read from its first cell: a row has one
    /// height, and the resize writes the same number to all of them.
    fn row_weight(&self, row: &[ZoneId]) -> f32 {
        row.first().map(|id| self.row_flex_of(*id)).unwrap_or(1.0)
    }

    /// Arm the drag when `(row, col)` sits on a border that separates two zones: the top edge
    /// of any row but the first, or the left edge of any cell but the first.
    pub fn resize_start(&mut self, row: u16, col: u16) -> bool {
        let kind = self.resize_target(row, col);
        self.resize = kind;
        kind.is_some()
    }

    /// Follow the pointer while a drag is armed.
    pub fn resize_drag(&mut self, row: u16, col: u16) {
        match self.resize {
            Some(ResizeKind::Row { above, below }) => self.drag_row(above, below, row),
            Some(ResizeKind::Col { left, right }) => self.drag_col(left, right, col),
            None => {}
        }
    }

    /// The pointer let go: nothing is being dragged any more, and the
    /// weights it left behind stay.
    pub fn resize_end(&mut self) {
        self.resize = None;
    }

    fn resize_target(&self, row: u16, col: u16) -> Option<ResizeKind> {
        let rows = self.visible_rows();
        for (i, cells) in rows.iter().enumerate() {
            for (j, &id) in cells.iter().enumerate() {
                let a = self.get_area(id);
                if a.width == 0 || a.height == 0 {
                    continue;
                }
                if row < a.y || row >= a.y + a.height || col < a.x || col >= a.x + a.width {
                    continue;
                }
                if j > 0 && col == a.x {
                    return Some(ResizeKind::Col {
                        left: cells[j - 1],
                        right: id,
                    });
                }
                if i > 0 && row == a.y {
                    return Some(ResizeKind::Row {
                        above: rows[i - 1][0],
                        below: cells[0],
                    });
                }
            }
        }
        None
    }

    fn drag_row(&mut self, above: ZoneId, below: ZoneId, pointer_row: u16) {
        let rows = self.visible_rows();
        let i = rows.iter().position(|cells| cells.contains(&above));
        let j = rows.iter().position(|cells| cells.contains(&below));
        let (Some(i), Some(j)) = (i, j) else {
            return;
        };
        if j != i + 1 {
            return;
        }
        let first = self.get_area(rows[i][0]);
        let second = self.get_area(rows[j][0]);
        let (start, end) = (first.y, second.y + second.height);
        if end.saturating_sub(start) < RESIZE_MIN_HEIGHT * 2 {
            return;
        }
        let target = pointer_row.clamp(start + RESIZE_MIN_HEIGHT, end - RESIZE_MIN_HEIGHT);

        // Every row states its own height as its weight; the pair split
        // at the pointer keeps their shared total, so the weights still
        // add up to the space and the untouched rows come out unchanged.
        for (k, cells) in rows.iter().enumerate() {
            let h = if k == i {
                target - start
            } else if k == j {
                end - target
            } else {
                self.get_area(cells[0]).height
            };
            for &id in cells {
                if let Some(zone) = self.zones.iter_mut().find(|z| z.id == id) {
                    zone.row_flex = f32::from(h);
                }
            }
        }
    }

    fn drag_col(&mut self, left: ZoneId, right: ZoneId, pointer_col: u16) {
        let rows = self.visible_rows();
        let Some(cells) = rows
            .into_iter()
            .find(|cells| cells.contains(&left) && cells.contains(&right))
        else {
            return;
        };
        let (Some(i), Some(j)) = (
            cells.iter().position(|&id| id == left),
            cells.iter().position(|&id| id == right),
        ) else {
            return;
        };
        if j != i + 1 {
            return;
        }
        let first = self.get_area(left);
        let second = self.get_area(right);
        let (start, end) = (first.x, second.x + second.width);
        if end.saturating_sub(start) < RESIZE_MIN_WIDTH * 2 {
            return;
        }
        let target = pointer_col.clamp(start + RESIZE_MIN_WIDTH, end - RESIZE_MIN_WIDTH);

        for (k, &id) in cells.iter().enumerate() {
            let w = if k == i {
                target - start
            } else if k == i + 1 {
                end - target
            } else {
                self.get_area(id).width
            };
            if let Some(zone) = self.zones.iter_mut().find(|z| z.id == id) {
                zone.flex = f32::from(w);
            }
        }
    }

    fn set_area(&mut self, id: ZoneId, area: Rect) {
        if let Some(zone) = self.zones.iter_mut().find(|z| z.id == id) {
            zone.area = area;
        }
    }

    pub fn get_area(&self, id: ZoneId) -> Rect {
        self.zones
            .iter()
            .find(|z| z.id == id)
            .map(|z| z.area)
            .unwrap_or_default()
    }

    pub fn is_visible(&self, id: ZoneId) -> bool {
        self.zones
            .iter()
            .find(|z| z.id == id)
            .map(|z| z.visible)
            .unwrap_or(false)
    }
}

/// Split `total` cells over `weights`: exact shares floored, then the leftover cells handed to
/// the shares that lost the most to flooring (ties in list order -- which is what keeps an
/// equal split identical to the old `total / n` plus "the first rows take the remainder").
fn distribute(total: u16, weights: &[f32]) -> Vec<u16> {
    let count = weights.len();
    if count == 0 {
        return Vec::new();
    }
    let sum: f32 = weights.iter().sum();
    if !sum.is_finite() || sum <= 0.0 {
        return distribute(total, &vec![1.0; count]);
    }

    let exact: Vec<f32> = weights
        .iter()
        .map(|w| f32::from(total) * (w / sum))
        .collect();
    let mut out: Vec<u16> = exact
        .iter()
        .map(|e| e.floor().clamp(0.0, f32::from(total)) as u16)
        .collect();

    let left = total.saturating_sub(out.iter().sum());
    if left > 0 {
        let mut order: Vec<usize> = (0..count).collect();
        order.sort_by(|&a, &b| {
            let lost = |i: usize| exact[i] - exact[i].floor();
            lost(b)
                .partial_cmp(&lost(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for &i in order.iter().take(left as usize) {
            out[i] += 1;
        }
    }
    out
}

pub fn superscript_digit(n: u8) -> &'static str {
    match n {
        0 => "\u{2070}",
        1 => "\u{00B9}",
        2 => "\u{00B2}",
        3 => "\u{00B3}",
        4 => "\u{2074}",
        5 => "\u{2075}",
        6 => "\u{2076}",
        7 => "\u{2077}",
        8 => "\u{2078}",
        9 => "\u{2079}",
        _ => "?",
    }
}

/// Which edge of a zone's frame carries a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameSlot {
    /// Top border, left, immediately after the zone title.
    TopLeft,
    /// Top border, right aligned (clipped away when the zone is narrow,
    /// exactly like btop's `if (width > 60 + sort_len)` guards).
    TopRight,
    /// Bottom border, left aligned.
    BottomLeft,
}

/// One function drawn on a zone's frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameButton {
    pub slot: FrameSlot,
    /// The character that triggers it.
    pub key: char,
    /// The word shown on the frame, e.g. "filter".
    pub label: String,
}

impl FrameButton {
    /// What actually gets drawn: `label` when it already contains the hotkey, otherwise `label`
    /// followed by the key.
    pub fn text(&self) -> String {
        if self.is_category() {
            return self.label.clone();
        }
        if self.label.contains(self.key) {
            self.label.clone()
        } else {
            format!("{} {}", self.label, self.key)
        }
    }

    /// Byte offset of the hotkey inside [`FrameButton::text`].
    pub fn hotkey_index(&self) -> usize {
        self.text().find(self.key).unwrap_or(0)
    }

    /// Drawn width in columns.
    pub fn width(&self) -> u16 {
        self.text().chars().count() as u16
    }

    /// Whether this is the Results category button, which carries btop's
    /// `◀ name ▶` sort-header arrows: the two arrow cells are separate
    /// mouse targets (previous / next category) and the name between
    /// them is not a target at all.
    pub fn is_category(&self) -> bool {
        self.label.starts_with('◀')
    }
}

/// Frame buttons per zone.
const RESULTS_BUTTONS: &[(FrameSlot, char, &str)] = &[
    // Lowercase `f`: the filter is the Results panel's primary function,
    // so it gets the letter unshifted while fullscreen -- which used to
    // hold `f` -- moved to `F` (btop capitalises a word when the hotkey
    // is uppercase; here the shift is what tells the two apart). It sits
    // with `group` on the right because both are state toggles, leaving
    // the left of the border to the title and the row counter.
    (FrameSlot::TopRight, 'f', "filter"),
    (FrameSlot::TopRight, 'g', "group"),
    // The bottom action row (`play ⏎` / `download d` / `info v`) is
    // gone: those three are keyboard-and-help-page actions now, and a
    // frame legend that repeats them is a second place to document the
    // same keys. The frame keeps the two that switch *state* (filter,
    // group); the rest are in `?`.
];

const TORRENT_BUTTONS: &[(FrameSlot, char, &str)] = &[
    (FrameSlot::TopRight, 'p', "pause"),
    (FrameSlot::BottomLeft, 'd', "delete"),
];

/// The buttons drawn on `id`'s frame; empty for zones with no actions.
pub fn zone_buttons(id: ZoneId) -> Vec<FrameButton> {
    let table: &[(FrameSlot, char, &str)] = match id {
        ZoneId::Results => RESULTS_BUTTONS,
        ZoneId::Torrent => TORRENT_BUTTONS,
        ZoneId::Log => &[],
        ZoneId::Trackers => &[],
    };
    table
        .iter()
        .map(|&(slot, key, label)| FrameButton {
            slot,
            key,
            label: label.to_string(),
        })
        .collect()
}

/// The zone's own title, btop `createBox` style: superscript number in `secondary` + bold,
/// label in `primary` (`btop_draw.cpp:290` for the numbering colour, `:332` for where it is
/// drawn).
pub fn zone_title(id: ZoneId, theme: &Theme, focused: bool) -> Line<'static> {
    let word = Style::default().fg(theme.primary_color());
    let number = Style::default()
        .fg(theme.secondary_color())
        .add_modifier(Modifier::BOLD);
    let mut spans = vec![Span::styled(
        if focused { "▸ " } else { "  " },
        if focused {
            word.add_modifier(Modifier::BOLD)
        } else {
            word
        },
    )];
    spans.extend([
        Span::styled(superscript_digit(id as u8), number),
        Span::styled(" ", word),
    ]);
    match id.detail_key() {
        Some(key) => {
            let hot = Style::default()
                .fg(theme.on_hover_color())
                .add_modifier(Modifier::BOLD);
            let rest = id.label().chars().skip(1);
            spans.push(Span::styled(key.to_string(), hot));
            spans.push(Span::styled(rest.collect::<String>(), word));
        }
        None => spans.push(Span::styled(id.label(), word)),
    }
    spans.push(Span::styled(" ", word));
    Line::from(spans)
}

/// Columns [`zone_title`] occupies, so the frame row starts right after it: space + superscript
/// + space + label + space.
pub fn zone_title_width(id: ZoneId) -> u16 {
    (5 + id.label().chars().count()) as u16
}

/// Spans for one button: `primary` for the word, `on_hover` + bold for the hotkey -- the glyph
/// that acts is coloured the way a hover marks the actionable part.
pub fn button_spans(
    theme: &Theme,
    button: &FrameButton,
    active: bool,
    hovered: bool,
) -> Vec<Span<'static>> {
    let text = button.text();
    let word_style = Style::default().fg(theme.primary_color());
    let hotkey_style = Style::default()
        .fg(theme.on_hover_color())
        .add_modifier(Modifier::BOLD);

    if button.is_category() {
        // `◀ name ▶`: the arrows are the targets, the name is not. The
        // spaces around the name are part of the label, so they stay.
        let name = text
            .trim_start_matches('◀')
            .trim_end_matches('▶')
            .to_string();
        return vec![
            Span::styled("◀", hotkey_style),
            Span::styled(
                name,
                if hovered {
                    hover_word(theme)
                } else {
                    word_style
                },
            ),
            Span::styled("▶", hotkey_style),
        ];
    }

    let idx = button.hotkey_index();
    let key_len = button.key.len_utf8();

    // A hovered button is underlined as well as tinted: the colour is
    // what the theme decided, and a theme whose `on_hover` happens to
    // sit close to `primary` would leave the pointer's position unreadable
    // -- which is the same "one channel is not enough" problem the focus
    // marker solves for zones. Underline is a shape, not a colour.
    let mut style = if hovered {
        hover_word(theme)
    } else {
        word_style
    };
    if active || hovered {
        style = style.add_modifier(Modifier::BOLD);
    }

    // The hotkey glyph joins the underline. It already carries `on_hover`
    // + bold, so leaving it out would draw a word underlined except for
    // the one letter that says what to press.
    let key_style = if hovered { style } else { hotkey_style };

    vec![
        Span::styled(text[..idx].to_string(), style),
        Span::styled(text[idx..idx + key_len].to_string(), key_style),
        Span::styled(text[idx + key_len..].to_string(), style),
    ]
}

fn hover_word(theme: &Theme) -> Style {
    Style::default()
        .fg(theme.on_hover_color())
        .add_modifier(Modifier::UNDERLINED)
}

/// The frame's colour: `primary` for the zone the cursor is in,
/// `div_line` for the rest.
pub fn zone_border_color(id: ZoneId, focused: ZoneId, theme: &Theme) -> Color {
    if id == focused {
        theme.primary_color()
    } else {
        theme.div_line.to_color()
    }
}

#[cfg(test)]
mod layout_math_tests {
    use super::*;

    /// The rule the layout had before it had weights, stated as a test: `total / n` with the
    /// first `total % n` cells taking one more.
    #[test]
    fn equal_weights_split_exactly_like_integer_division() {
        for total in 0..60u16 {
            for n in 1..8usize {
                let row = total / n as u16;
                let rem = total - row * n as u16;
                let want: Vec<u16> = (0..n).map(|i| row + u16::from((i as u16) < rem)).collect();
                assert_eq!(
                    distribute(total, &vec![1.0; n]),
                    want,
                    "total={total} n={n}"
                );
            }
        }
    }

    /// The one invariant a split must never break: what comes out adds up to what went in.
    #[test]
    fn the_shares_always_add_up_to_the_total() {
        let cases = [
            vec![1.0],
            vec![1.0, 1.0, 1.0, 1.0],
            vec![0.5, 3.0, 1.0],
            vec![10.0, 0.0],
            vec![0.0, 0.0],
            vec![0.1; 7],
            vec![1000.0, 1.0, 1.0],
            vec![f32::NAN],
        ];
        for weights in cases {
            for total in [0u16, 1, 2, 7, 40, 101, 400] {
                let out = distribute(total, &weights);
                assert_eq!(out.len(), weights.len(), "one share per cell");
                assert_eq!(
                    out.iter().sum::<u16>(),
                    total,
                    "weights={weights:?} total={total}"
                );
            }
        }
    }

    #[test]
    fn a_bigger_weight_gets_the_bigger_cell() {
        assert_eq!(distribute(100, &[1.0, 3.0]), vec![25, 75]);
        assert_eq!(distribute(97, &[1.0, 3.0]), vec![24, 73]);
    }

    #[test]
    fn equal_shares_of_every_size_add_up() {
        assert_eq!(distribute(3, &[1.0, 1.0, 1.0]), vec![1, 1, 1]);
        assert_eq!(distribute(33, &[1.0, 1.0, 1.0]), vec![11, 11, 11]);
        assert_eq!(distribute(49, &[1.0; 7]), vec![7; 7]);
        assert_eq!(distribute(12, &[1.0; 6]), vec![2; 6]);
    }

    /// The float case worth pinning: `1.0f32 / 41.0` rounds *below* `1/41`, so `41 * that` is
    /// `0.99999994` and every share floors to zero -- 41 cells to hand out and none of them
    /// taken.
    #[test]
    fn a_share_lost_to_float_rounding_comes_back_as_a_remainder() {
        assert_eq!(distribute(41, &[1.0; 41]), vec![1; 41]);
        assert_eq!(distribute(82, &[1.0; 41]), vec![2; 41]);
        assert_eq!(distribute(40, &[1.0; 41]), {
            // Fewer cells than shares: 40 ones and one zero, at the end.
            let mut want = vec![1u16; 41];
            want[40] = 0;
            want
        });
    }

    /// More cells than rows to give: the split degrades to a mix of 0s
    /// and 1s rather than overflowing -- and it still adds up.
    #[test]
    fn fewer_cells_than_zones_still_adds_up() {
        assert_eq!(
            distribute(2, &[1.0, 1.0, 1.0, 1.0, 1.0]),
            vec![1, 1, 0, 0, 0]
        );
        assert_eq!(distribute(0, &[1.0, 1.0]), vec![0, 0]);
    }
}
