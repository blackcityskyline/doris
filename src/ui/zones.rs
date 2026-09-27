use ratatui::prelude::*;
use super::theme::Theme;

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
    Log = 3,
    Extra = 4,
}

impl ZoneId {
    pub fn all() -> &'static [ZoneId] {
        &[ZoneId::Results, ZoneId::Torrent, ZoneId::Log, ZoneId::Extra]
    }

    pub fn key_char(&self) -> char {
        match self {
            ZoneId::Results => '1',
            ZoneId::Torrent => '2',
            ZoneId::Log => '3',
            ZoneId::Extra => '4',
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ZoneId::Results => "Results",
            ZoneId::Torrent => "Torrent",
            ZoneId::Log => "Log",
            ZoneId::Extra => "Extra",
        }
    }

    pub fn from_key(c: char) -> Option<ZoneId> {
        match c {
            '1' => Some(ZoneId::Results),
            '2' => Some(ZoneId::Torrent),
            '3' => Some(ZoneId::Log),
            '4' => Some(ZoneId::Extra),
            _ => None,
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
}

impl ZoneLayout {
    pub fn new() -> Self {
        Self {
            zones: vec![
                Zone::new(ZoneId::Results, true),
                Zone::new(ZoneId::Torrent, true),
                Zone::new(ZoneId::Log, true),
                Zone::new(ZoneId::Extra, false),
            ],
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
    /// `"1,3"` shows only Results and Log and hides Torrent/Extra. Unknown
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
        let visible: Vec<ZoneId> = self.zones.iter()
            .filter(|z| z.visible)
            .map(|z| z.id)
            .collect();
        if visible.is_empty() { return; }
        if let Some(pos) = visible.iter().position(|&z| z == self.focused) {
            let next = (pos + 1) % visible.len();
            self.focused = visible[next];
        } else {
            self.focused = visible[0];
        }
    }

    pub fn focus_prev(&mut self) {
        let visible: Vec<ZoneId> = self.zones.iter()
            .filter(|z| z.visible)
            .map(|z| z.id)
            .collect();
        if visible.is_empty() { return; }
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
                zone.area = if zone.id == fs_id { area } else { Rect::default() };
            }
            return;
        }

        let visible_zones: Vec<ZoneId> = self.zones.iter()
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

    fn set_area(&mut self, id: ZoneId, area: Rect) {
        if let Some(zone) = self.zones.iter_mut().find(|z| z.id == id) {
            zone.area = area;
        }
    }

    pub fn get_area(&self, id: ZoneId) -> Rect {
        self.zones.iter()
            .find(|z| z.id == id)
            .map(|z| z.area)
            .unwrap_or_default()
    }

    pub fn is_visible(&self, id: ZoneId) -> bool {
        self.zones.iter()
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameButton {
    pub slot: FrameSlot,
    /// The character that triggers it. A glyph such as `⏎` stands in for
    /// a non-text key, so this is always exactly one column wide.
    pub key: char,
    /// The word shown on the frame, e.g. "filter".
    pub label: &'static str,
}

impl FrameButton {
    /// What actually gets drawn: `label` when it already contains the
    /// hotkey, otherwise `label` followed by the key.
    pub fn text(&self) -> String {
        if self.label.contains(self.key) {
            self.label.to_string()
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
}

/// Frame buttons per zone.
///
/// The keys are the bindings in AGENTS.md; a word is picked so its first
/// letter is free for the hotkey whenever possible (`f` belongs to
/// fullscreen, so filter has to take `F`; `v`/`]`/`⏎` are not letters at
/// all and end up trailing the word). Kept next to `zone_title` so the
/// legend and the bindings it advertises are edited together.
const RESULTS_BUTTONS: &[FrameButton] = &[
    // Capital `F` because the key is shift-F: `f` already belongs to
    // fullscreen, and btop capitalises the word the same way when the
    // hotkey is uppercase (`Nice`, `Follow`).
    FrameButton { slot: FrameSlot::TopLeft, key: 'F', label: "Filter" },
    FrameButton { slot: FrameSlot::TopRight, key: 'g', label: "group" },
    FrameButton { slot: FrameSlot::TopRight, key: ']', label: "source" },
    FrameButton { slot: FrameSlot::BottomLeft, key: '⏎', label: "play" },
    FrameButton { slot: FrameSlot::BottomLeft, key: 'd', label: "download" },
    FrameButton { slot: FrameSlot::BottomLeft, key: 'v', label: "info" },
];

const TORRENT_BUTTONS: &[FrameButton] = &[
    FrameButton { slot: FrameSlot::TopRight, key: 'p', label: "pause" },
    FrameButton { slot: FrameSlot::BottomLeft, key: 'd', label: "delete" },
];

/// The Log panel's only real action: jump to the full-screen detail log.
const LOG_BUTTONS: &[FrameButton] = &[FrameButton {
    slot: FrameSlot::TopRight,
    key: 'L',
    label: "detail",
}];

/// The buttons drawn on `id`'s frame; empty for zones with no actions.
pub fn zone_buttons(id: ZoneId) -> &'static [FrameButton] {
    match id {
        ZoneId::Results => RESULTS_BUTTONS,
        ZoneId::Torrent => TORRENT_BUTTONS,
        ZoneId::Log => LOG_BUTTONS,
        ZoneId::Extra => &[],
    }
}

/// The zone's own title, btop `createBox` style: superscript number in
/// `hi_fg` + bold, label in `title` (`btop_draw.cpp:290` for the
/// numbering colour, `:332` for where it is drawn).
pub fn zone_title(id: ZoneId, theme: &Theme) -> Line<'static> {
    let word = Style::default().fg(theme.title.to_color());
    let number = Style::default()
        .fg(theme.hi_fg.to_color())
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

/// Spans for one button: `title` for the word, `hi_fg` + bold for the
/// hotkey. `active` bolds the whole word, which is how btop marks a
/// toggle that is currently on (`Fx::b` around `pause` when
/// `pause_proc_list`, around `tree` when `proc_tree`, ...).
pub fn button_spans(theme: &Theme, button: &FrameButton, active: bool) -> Vec<Span<'static>> {
    let text = button.text();
    let idx = button.hotkey_index();
    let key_len = button.key.len_utf8();

    let mut word_style = Style::default().fg(theme.title.to_color());
    if active {
        word_style = word_style.add_modifier(Modifier::BOLD);
    }
    let hotkey_style = Style::default()
        .fg(theme.hi_fg.to_color())
        .add_modifier(Modifier::BOLD);

    vec![
        Span::styled(text[..idx].to_string(), word_style),
        Span::styled(text[idx..idx + key_len].to_string(), hotkey_style),
        Span::styled(text[idx + key_len..].to_string(), word_style),
    ]
}

pub fn zone_border_color(id: ZoneId, focused: ZoneId, theme: &Theme) -> Color {
    if id == focused {
        theme.hi_fg.to_color()
    } else {
        theme.div_line.to_color()
    }
}
