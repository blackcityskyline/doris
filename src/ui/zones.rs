use super::theme::Theme;
use ratatui::prelude::*;

/// Rows the always-visible search input takes: top border, one line of
/// text, bottom border. `update_areas` splits it off the top of the
/// terminal, `App::render_search_bar` draws into it and
/// `App::search_box_at` recognises it -- one number for all three, so
/// the zones can never drift under the box.
pub const SEARCH_BAR_HEIGHT: u16 = 3;

/// Smallest height the split layout gives the Results band. A third of
/// a short terminal is often less than a legend and a row, and a zone
/// that short is a box with nothing in it.
const MIN_SPLIT_ROWS: u16 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneId {
    Results = 1,
    Torrent = 2,
    Log = 3,
    /// The sources checklist (П.4): which sources the search asks. It
    /// took the number the "Extra" placeholder used to hold, so the
    /// zone a user actually touches sits at `4` instead of hiding
    /// behind a fifth key of a panel that drew nothing.
    Sources = 4,
}

/// `(digit key, label)` per zone. One row per [`ZoneId`] -- the three
/// lookups below all read it, so adding a zone means appending a variant
/// and a row instead of editing three matches.
/// The render/navigation dispatch on `ZoneId` in `ui/app.rs` cannot be
/// table-driven: each zone draws different state.
const ZONE_ROWS: &[(ZoneId, char, &str)] = &[
    (ZoneId::Results, '1', "Results"),
    (ZoneId::Torrent, '2', "Torrent"),
    (ZoneId::Log, '3', "Log"),
    (ZoneId::Sources, '4', "Sources"),
];

impl ZoneId {
    pub fn all() -> &'static [ZoneId] {
        &[
            ZoneId::Results,
            ZoneId::Torrent,
            ZoneId::Log,
            ZoneId::Sources,
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
    /// How the visible zones are arranged below the search bar. Session
    /// only -- nothing in Config asks for a layout yet, so there is
    /// nothing to persist.
    pub preset: LayoutPreset,
}

/// How the visible zones share the screen below the search bar.
///
/// The search bar itself is not part of any preset: it is always the top
/// [`SEARCH_BAR_HEIGHT`] rows at full width, which is what "the search
/// is always there" means in practice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutPreset {
    /// Every visible zone spans the full width and the height is shared
    /// equally -- the layout there has been since the zones existed.
    #[default]
    Horizontal,
    /// Results on top at full width, then two columns: Torrent on the
    /// left, Log and Sources stacked on the right. A zone that is turned
    /// off gives its space to whatever shares its column.
    Split,
}

impl LayoutPreset {
    /// The next preset, for the key that cycles them. Two presets today,
    /// so this is a toggle -- a third one would only need its own arm.
    pub fn next(self) -> Self {
        match self {
            LayoutPreset::Horizontal => LayoutPreset::Split,
            LayoutPreset::Split => LayoutPreset::Horizontal,
        }
    }
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
                Zone::new(ZoneId::Log, true),
                Zone::new(ZoneId::Sources, true),
            ],
            preset: LayoutPreset::default(),
            focused: ZoneId::Results,
            fullscreen: None,
            filter_mode: false,
            filter_input: String::new(),
        }
    }

    /// Switch to the next layout preset (`P`). The areas are recomputed
    /// on the next `update_areas`, which every render starts with, so
    /// this is a one-line change with no render of its own.
    pub fn cycle_preset(&mut self) {
        self.preset = self.preset.next();
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

    /// Apply a preset written as a comma-separated list of zone key
    /// characters (the same digits the 1/2/3/4 keybinds use), e.g.
    /// `"1,3"` shows only Results and Log and hides the rest. Unknown
    /// characters are ignored. If focus would land on a now-hidden zone,
    /// it moves to the first visible one.
    pub fn apply_preset(&mut self, spec: &str) {
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

        match self.preset {
            LayoutPreset::Horizontal => self.layout_horizontal(area),
            LayoutPreset::Split => self.layout_split(area),
        }
    }

    /// The default layout: every visible zone at full width, sharing the
    /// height equally, the first `remainder` of them one row taller so
    /// no row of the terminal is wasted.
    fn layout_horizontal(&mut self, area: Rect) {
        let visible_zones: Vec<ZoneId> = self
            .zones
            .iter()
            .filter(|z| z.visible)
            .map(|z| z.id)
            .collect();

        let count = visible_zones.len();
        if count == 0 {
            for zone in &mut self.zones {
                zone.area = Rect::default();
            }
            return;
        }

        let available = area.height.saturating_sub(SEARCH_BAR_HEIGHT);
        let zone_height = available / count as u16;
        let remainder = available.saturating_sub(zone_height * count as u16);

        let mut y = area.y + SEARCH_BAR_HEIGHT;
        for (i, &id) in visible_zones.iter().enumerate() {
            // Give the first `remainder` zones one extra row so every
            // row of the terminal is used.
            let h = zone_height + if (i as u16) < remainder { 1 } else { 0 };
            let zone_area = Rect::new(area.x, y, area.width, h);
            self.set_area(id, zone_area);
            y += h;
        }

        for zone in &mut self.zones {
            if !zone.visible {
                zone.area = Rect::default();
            }
        }
    }

    /// The split layout: Results on top at full width, then two columns
    /// -- Torrent on the left, Log and Sources stacked on the right.
    ///
    /// A zone that is turned off gives its space to whatever shares its
    /// column, and a column whose zones are all off gives its width to
    /// the other one, so hiding something never leaves a hole.
    fn layout_split(&mut self, area: Rect) {
        let mut y = area.y + SEARCH_BAR_HEIGHT;
        let mut remaining = area.height.saturating_sub(SEARCH_BAR_HEIGHT);

        // Results first: about a third of what is there, full width. The
        // floor keeps a legend and a row or two on screen at terminal
        // heights where a third of nothing is nothing.
        if self.is_visible(ZoneId::Results) {
            let h = ((remaining / 3).max(MIN_SPLIT_ROWS)).min(remaining);
            self.set_area(ZoneId::Results, Rect::new(area.x, y, area.width, h));
            y += h;
            remaining -= h;
        }

        let left_visible = self.is_visible(ZoneId::Torrent);
        let right_visible = self.is_visible(ZoneId::Log) || self.is_visible(ZoneId::Sources);
        let half = area.width / 2;
        let (left_w, right_w) = match (left_visible, right_visible) {
            (true, true) => (half, area.width - half),
            (true, false) => (area.width, 0),
            (false, true) => (0, area.width),
            (false, false) => (0, 0),
        };

        if left_visible {
            self.set_area(ZoneId::Torrent, Rect::new(area.x, y, left_w, remaining));
        }

        if right_visible {
            let right_x = area.x + left_w;
            // Log and Sources share the column; with one of them hidden
            // the other takes the whole of it.
            let column: Vec<ZoneId> = [ZoneId::Log, ZoneId::Sources]
                .into_iter()
                .filter(|id| self.is_visible(*id))
                .collect();
            let count = column.len() as u16;
            let h = remaining / count;
            let mut ry = y;
            for (i, id) in column.iter().enumerate() {
                // The last one takes the remainder, so the column adds
                // up to exactly `remaining` rows.
                let zh = if i == column.len() - 1 {
                    remaining - h * (count - 1)
                } else {
                    h
                };
                self.set_area(*id, Rect::new(right_x, ry, right_w, zh));
                ry += zh;
            }
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
    // is uppercase; here the shift is what tells the two apart).
    (FrameSlot::TopLeft, 'f', "Filter"),
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

/// The Log panel's only real action: jump to the full-screen detail log.
const LOG_BUTTONS: &[(FrameSlot, char, &str)] = &[(FrameSlot::TopRight, 'L', "detail")];

/// The buttons drawn on `id`'s frame; empty for zones with no actions.
///
/// The Sources panel has none on purpose: its rows are the actions, and
/// a frame legend would only repeat what `j`/`k` and Enter already say
/// (btop's proc panel draws its actions on the border because the rows
/// there are data, not controls).
pub fn zone_buttons(id: ZoneId) -> Vec<FrameButton> {
    let table: &[(FrameSlot, char, &str)] = match id {
        ZoneId::Results => RESULTS_BUTTONS,
        ZoneId::Torrent => TORRENT_BUTTONS,
        ZoneId::Log => LOG_BUTTONS,
        ZoneId::Sources => &[],
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
pub fn zone_title(id: ZoneId, theme: &Theme) -> Line<'static> {
    let word = Style::default().fg(theme.primary_color());
    let number = Style::default()
        .fg(theme.secondary_color())
        .add_modifier(Modifier::BOLD);
    Line::from(vec![
        Span::styled(" ", word),
        Span::styled(superscript_digit(id as u8), number),
        Span::styled(" ", word),
        Span::styled(id.label(), word),
        Span::styled(" ", word),
    ])
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
