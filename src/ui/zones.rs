use super::theme::Theme;
use ratatui::prelude::*;

/// Rows the always-visible search input takes: top border, one line of
/// text, bottom border. `update_areas` splits it off the top of the
/// terminal, `App::render_search_bar` draws into it and
/// `App::search_box_at` recognises it -- one number for all three, so
/// the zones can never drift under the box.
pub const SEARCH_BAR_HEIGHT: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneId {
    Results = 1,
    Torrent = 2,
    /// The trackers checklist (П.4): which sources the search asks.
    /// It took the number the "Extra" placeholder used to hold, so the
    /// zone a user actually touches sits at `3` instead of hiding
    /// behind a fifth key of a panel that drew nothing.
    Trackers = 3,
    /// The log moved to the last slot so the zone keyboard reads
    /// Results / Torrent / Trackers / Log in row order.
    Log = 4,
}

/// `(digit key, label)` per zone. One row per [`ZoneId`] -- the three
/// lookups below all read it, so adding a zone means appending a variant
/// and a row instead of editing three matches.
/// The render/navigation dispatch on `ZoneId` in `ui/app.rs` cannot be
/// table-driven: each zone draws different state.
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

    /// The key that takes over the frame with this zone's detail view
    /// (`L`/`T`/`R`), or `None` for a zone that has none. It is the
    /// label's first letter, which is what lets `zone_title` highlight
    /// it without a second table to keep in step.
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
}

impl Zone {
    pub fn new(id: ZoneId, visible: bool) -> Self {
        Self {
            id,
            visible,
            area: Rect::default(),
        }
    }
}

pub struct ZoneLayout {
    pub zones: Vec<Zone>,
    pub focused: ZoneId,
    pub fullscreen: Option<ZoneId>,
    pub filter_mode: bool,
    pub filter_input: String,
    /// The tiling: rows of cells, one cell per zone, left to right and
    /// top to bottom. Written as `rows via ","`, `columns via "|"` --
    /// see [`ZoneLayout::apply_preset`].
    pub grid: Vec<Vec<ZoneId>>,
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
    ///
    /// A zone the user is not standing in is a zone they want to look
    /// at, so the first press *focuses* it (showing it if it was
    /// hidden); only the zone already under the focus is taken away --
    /// and when it goes, the focus walks to the next zone still on
    /// screen instead of parking somewhere nobody can see.
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

    /// Set a zone's visibility directly, rather than flipping it. Used by
    /// [`apply_preset`](Self::apply_preset) so a preset can show exactly
    /// the zones it names instead of toggling from an unknown starting
    /// state.
    pub fn set_visible(&mut self, id: ZoneId, visible: bool) {
        if let Some(zone) = self.zones.iter_mut().find(|z| z.id == id) {
            zone.visible = visible;
        }
        if !visible && self.fullscreen == Some(id) {
            self.fullscreen = None;
        }
    }

    /// Apply a tiling spec. Rows are separated by `,`, columns inside a
    /// row by `|`, and every zone is named by its 1/2/3/4 key digit, so
    /// `"1,3|4"` is the default UI: Results alone across the top, then
    /// Trackers beside Log. A cell may hold several digits (`"34"` reads
    /// as `"3|4"`), and any character that is not a zone key is ignored,
    /// which keeps the old flat `"1,2,3,4"` meaning exactly what it
    /// always did -- four rows, one zone each. If focus would land on a
    /// now-hidden zone, it moves to the first visible one.
    pub fn apply_preset(&mut self, spec: &str) {
        self.grid = Self::parse_spec(spec);
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

    /// Split a spec into rows of zone keys: `,` starts a new row, and
    /// the digits inside one chunk are its cells left to right. Empty
    /// rows (a chunk with no zone key in it) are dropped.
    fn parse_spec(spec: &str) -> Vec<Vec<ZoneId>> {
        spec.split(',')
            .map(|row| row.chars().filter_map(ZoneId::from_key).collect())
            .filter(|row: &Vec<ZoneId>| !row.is_empty())
            .collect()
    }

    /// The grid: rows from `grid` top to bottom, cells left to right,
    /// each row taking an equal share of the height below the search
    /// bar and each cell an equal share of its row's width -- the
    /// first row and the first cells of a row taking the remainder, so
    /// no row of the terminal is wasted.
    ///
    /// A cell whose zone is switched off is dropped, giving its width
    /// to the rest of its row; a row that ends up empty is dropped,
    /// giving its height to the rows that remain. A visible zone the
    /// spec never named (switched back on with its digit key) gets a
    /// row of its own at the bottom rather than vanishing.
    fn layout_grid(&mut self, area: Rect) {
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

        if rows.is_empty() {
            for zone in &mut self.zones {
                zone.area = Rect::default();
            }
            return;
        }

        let available = area.height.saturating_sub(SEARCH_BAR_HEIGHT);
        let row_height = available / rows.len() as u16;
        let row_remainder = available - row_height * rows.len() as u16;

        let mut y = area.y + SEARCH_BAR_HEIGHT;
        for (i, row) in rows.iter().enumerate() {
            let h = row_height + if (i as u16) < row_remainder { 1 } else { 0 };
            let cell_width = area.width / row.len() as u16;
            let cell_remainder = area.width - cell_width * row.len() as u16;
            let mut x = area.x;
            for (j, &id) in row.iter().enumerate() {
                let w = cell_width + if (j as u16) < cell_remainder { 1 } else { 0 };
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
///
/// btop draws every panel action on the border line itself instead of
/// inside the box: `filter` right after the box title
/// (`btop_draw.cpp:1902`), `pause`/`per-core`/`reverse`/`tree` right
/// aligned on the top border (`:1915`-`:1940`), and
/// `terminate`/`kill`/`signals`/`Nice`/`follow` running along the
/// bottom one (`:1956`-`:1979`). The border doubles as the keybind
/// legend, which is why the zone body itself can stay clean.
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
///
/// The word is `title` colour and the character that triggers it is
/// `hi_fg` + bold: the highlight marks the hotkey, not the alphabet --
/// btop spells it "pa**u**se" (`:1923`) precisely because `p` was taken,
/// and renders "info ⏎" (`:1956`) where the key is a glyph rather than a
/// letter. Clicking the word fires the same action as pressing the key
/// (btop registers both: the spans above and `Input::mouse_mappings`).
///
/// `label` is a `String` rather than `&'static str` because one button
/// -- the Results category -- names the thing it switches, and that
/// changes as the category does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameButton {
    pub slot: FrameSlot,
    /// The character that triggers it. A glyph such as `⏎` stands in for
    /// a non-text key, so this is always exactly one column wide.
    pub key: char,
    /// The word shown on the frame, e.g. "filter".
    pub label: String,
}

impl FrameButton {
    /// What actually gets drawn: `label` when it already contains the
    /// hotkey, otherwise `label` followed by the key.
    ///
    /// The category button is the exception: it is mouse-only (its two
    /// arrows are the targets), so no key is appended -- the `g` keybind
    /// it shares with `group` is advertised on that button instead.
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
///
/// The keys are the bindings in AGENTS.md; a word is picked so its first
/// letter is free for the hotkey whenever possible (`f` filters and `F`
/// goes fullscreen, so in both cases the label leads with the binding).
/// Kept next to `zone_title` so the legend and the bindings it
/// advertises are edited together.
///
/// The tables are `(slot, key, label)` tuples rather than `FrameButton`s
/// so they stay `const` -- only the category button has a dynamic label,
/// and it is built in `frame_layout`, not here.
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
///
/// The Trackers panel has none on purpose: its rows are the actions, and
/// a frame legend would only repeat what `j`/`k` and Enter already say
/// (btop's proc panel draws its actions on the border because the rows
/// there are data, not controls).
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

/// The zone's own title, btop `createBox` style: superscript number in
/// `secondary` + bold, label in `primary` (`btop_draw.cpp:290` for the
/// numbering colour, `:332` for where it is drawn).
///
/// The label leads with its detail-view key (`L`/`T`/`R`), drawn the
/// way a frame button draws its hotkey -- `on_hover` + bold -- so the
/// letter that opens the full-frame takeover is visible where the zone
/// is. Trackers has no detail view, so its label stays one plain span.
pub fn zone_title(id: ZoneId, theme: &Theme) -> Line<'static> {
    let word = Style::default().fg(theme.primary_color());
    let number = Style::default()
        .fg(theme.secondary_color())
        .add_modifier(Modifier::BOLD);
    let mut spans = vec![
        Span::styled(" ", word),
        Span::styled(superscript_digit(id as u8), number),
        Span::styled(" ", word),
    ];
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

/// Columns [`zone_title`] occupies, so the frame row starts right after
/// it: space + superscript + space + label + space.
pub fn zone_title_width(id: ZoneId) -> u16 {
    (4 + id.label().chars().count()) as u16
}

/// Spans for one button: `primary` for the word, `on_hover` + bold for
/// the hotkey -- the glyph that acts is coloured the way a hover marks
/// the actionable part. `active` bolds the whole word, which is how
/// btop marks a toggle that is currently on (`Fx::b` around `pause`
/// when `pause_proc_list`, around `tree` when `proc_tree`, ...).
///
/// The category button is the exception: it has no single hotkey, but
/// two arrow cells that are mouse targets, so both arrows take the
/// `on_hover` + bold treatment and the name between them stays
/// `primary` -- btop draws its sortable column headers the same way
/// (`◀ name ▶`).
pub fn button_spans(theme: &Theme, button: &FrameButton, active: bool) -> Vec<Span<'static>> {
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
            Span::styled(name, word_style),
            Span::styled("▶", hotkey_style),
        ];
    }

    let idx = button.hotkey_index();
    let key_len = button.key.len_utf8();

    let mut style = word_style;
    if active {
        style = style.add_modifier(Modifier::BOLD);
    }

    vec![
        Span::styled(text[..idx].to_string(), style),
        Span::styled(text[idx..idx + key_len].to_string(), hotkey_style),
        Span::styled(text[idx + key_len..].to_string(), style),
    ]
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
