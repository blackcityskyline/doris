use ratatui::prelude::*;
use super::theme::Theme;

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

    pub fn label(&self) -> &str {
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

        let search_bar_height: u16 = 3;
        let log_height: u16 = 8;
        let torrent_height: u16 = 8;

        let has_torrent = visible_zones.contains(&ZoneId::Torrent);
        let has_log = visible_zones.contains(&ZoneId::Log);

        let bottom_height: u16 = if has_log { log_height } else { 0 }
            + if has_torrent { torrent_height } else { 0 };

        let remaining = area.height.saturating_sub(search_bar_height + bottom_height);
        let results_height = remaining.max(5);

        let mut y = area.y + search_bar_height;

        for &id in &visible_zones {
            match id {
                ZoneId::Results => {
                    let h = results_height;
                    let area = Rect::new(area.x, y, area.width, h);
                    self.set_area(id, area);
                    y += h;
                }
                ZoneId::Torrent => {
                    let area = Rect::new(area.x, y, area.width, torrent_height);
                    self.set_area(id, area);
                    y += torrent_height;
                }
                ZoneId::Log => {
                    let area = Rect::new(area.x, y, area.width, log_height);
                    self.set_area(id, area);
                    y += log_height;
                }
                ZoneId::Extra => {
                    let area = Rect::new(area.x, y, area.width, remaining);
                    self.set_area(id, area);
                }
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

pub fn zone_title(id: ZoneId, _theme: &Theme) -> String {
    let num = id as u8;
    let sup = superscript_digit(num);
    format!(" {} {} ", sup, id.label())
}

pub fn zone_border_color(id: ZoneId, focused: ZoneId, theme: &Theme) -> Color {
    if id == focused {
        theme.hi_fg.to_color()
    } else {
        theme.div_line.to_color()
    }
}
