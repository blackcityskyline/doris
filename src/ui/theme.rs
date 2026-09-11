use ratatui::style::Color;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub main_bg: ColorDef,
    pub main_fg: ColorDef,
    pub title: ColorDef,
    pub hi_fg: ColorDef,
    pub selected_bg: ColorDef,
    pub selected_fg: ColorDef,
    pub inactive_fg: ColorDef,
    pub div_line: ColorDef,
    pub graph_text: ColorDef,
    pub meter_bg: ColorDef,
    pub search_box: ColorDef,
    pub log_box: ColorDef,
    pub player_box: ColorDef,
    pub menu_bg: ColorDef,
    pub menu_fg: ColorDef,
    pub menu_selected_bg: ColorDef,
    pub menu_selected_fg: ColorDef,
    pub gradient_start: ColorDef,
    pub gradient_mid: ColorDef,
    pub gradient_end: ColorDef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorDef {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl ColorDef {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn to_color(&self) -> Color {
        Color::Rgb(self.r, self.g, self.b)
    }
}

impl From<ColorDef> for Color {
    fn from(def: ColorDef) -> Self {
        def.to_color()
    }
}

pub fn gradient_array(start: &ColorDef, mid: &ColorDef, end: &ColorDef, len: usize) -> Vec<Color> {
    let mut result = Vec::with_capacity(len);
    let half = len / 2;
    for i in 0..len {
        let (from, to, t) = if i < half {
            (start, mid, i as f64 / half as f64)
        } else {
            (mid, end, (i - half) as f64 / (len - half).max(1) as f64)
        };
        let r = from.r as f64 + (to.r as f64 - from.r as f64) * t;
        let g = from.g as f64 + (to.g as f64 - from.g as f64) * t;
        let b = from.b as f64 + (to.b as f64 - from.b as f64) * t;
        result.push(Color::Rgb(r as u8, g as u8, b as u8));
    }
    result
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

/// Every theme shipped in the repo's top-level `themes/` directory,
/// embedded at compile time so they always work regardless of the
/// current working directory or install location. `load_themes()` used
/// to *only* look in `~/.config/doris/themes` (or `./themes` relative to
/// whatever directory the binary happened to be launched from) -- so
/// running the built binary from anywhere other than a checkout with
/// that directory manually populated found zero themes and silently
/// showed "Color theme 1/1". User-added files in
/// `~/.config/doris/themes/*.toml` are still loaded on top of this list
/// (and can override a bundled theme of the same name), so the
/// extensibility that directory was meant to provide isn't lost.
const BUNDLED_THEMES: &[&str] = &[
    include_str!("../../themes/HotPurpleTrafficLight.toml"),
    include_str!("../../themes/adapta.toml"),
    include_str!("../../themes/adwaita-dark.toml"),
    include_str!("../../themes/adwaita.toml"),
    include_str!("../../themes/ayu.toml"),
    include_str!("../../themes/default.toml"),
    include_str!("../../themes/dracula.toml"),
    include_str!("../../themes/dusklight.toml"),
    include_str!("../../themes/elementarish.toml"),
    include_str!("../../themes/everforest-dark-hard.toml"),
    include_str!("../../themes/everforest-dark-medium.toml"),
    include_str!("../../themes/everforest-light-medium.toml"),
    include_str!("../../themes/flat-remix-light.toml"),
    include_str!("../../themes/flat-remix.toml"),
    include_str!("../../themes/flexoki-dark.toml"),
    include_str!("../../themes/flexoki-light.toml"),
    include_str!("../../themes/gotham.toml"),
    include_str!("../../themes/greyscale.toml"),
    include_str!("../../themes/gruvbox_dark.toml"),
    include_str!("../../themes/gruvbox_dark_v2.toml"),
    include_str!("../../themes/gruvbox_light.toml"),
    include_str!("../../themes/gruvbox_material_dark.toml"),
    include_str!("../../themes/horizon.toml"),
    include_str!("../../themes/kanagawa-dragon.toml"),
    include_str!("../../themes/kanagawa-lotus.toml"),
    include_str!("../../themes/kanagawa-wave.toml"),
    include_str!("../../themes/kyli0x.toml"),
    include_str!("../../themes/matcha-dark-sea.toml"),
    include_str!("../../themes/monokai.toml"),
    include_str!("../../themes/night-owl.toml"),
    include_str!("../../themes/nord.toml"),
    include_str!("../../themes/onedark.toml"),
    include_str!("../../themes/orange.toml"),
    include_str!("../../themes/paper.toml"),
    include_str!("../../themes/phoenix-night.toml"),
    include_str!("../../themes/solarized_dark.toml"),
    include_str!("../../themes/solarized_light.toml"),
    include_str!("../../themes/tokyo-night.toml"),
    include_str!("../../themes/tokyo-storm.toml"),
    include_str!("../../themes/tomorrow-night.toml"),
    include_str!("../../themes/twilight.toml"),
    include_str!("../../themes/whiteout.toml"),
];

impl Theme {
    pub fn dark() -> Self {
        Self {
            name: "default".into(),
            main_bg: ColorDef::new(10, 22, 40),
            main_fg: ColorDef::new(200, 215, 225),
            title: ColorDef::new(240, 250, 255),
            hi_fg: ColorDef::new(79, 195, 247),
            selected_bg: ColorDef::new(20, 50, 80),
            selected_fg: ColorDef::new(128, 222, 234),
            inactive_fg: ColorDef::new(55, 85, 105),
            div_line: ColorDef::new(25, 60, 95),
            graph_text: ColorDef::new(100, 165, 195),
            meter_bg: ColorDef::new(15, 32, 55),
            search_box: ColorDef::new(79, 195, 247),
            log_box: ColorDef::new(38, 166, 154),
            player_box: ColorDef::new(0, 188, 212),
            menu_bg: ColorDef::new(10, 22, 40),
            menu_fg: ColorDef::new(144, 164, 174),
            menu_selected_bg: ColorDef::new(20, 50, 80),
            menu_selected_fg: ColorDef::new(128, 222, 234),
            gradient_start: ColorDef::new(0, 188, 212),
            gradient_mid: ColorDef::new(38, 166, 154),
            gradient_end: ColorDef::new(77, 182, 172),
        }
    }

    pub fn default_theme() -> Self {
        Self::dark()
    }

    pub fn from_config(path: &std::path::Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        Self::from_config_str(&content)
    }

    fn from_config_str(content: &str) -> Option<Self> {
        toml::from_str(content).ok()
    }

    pub fn load_themes() -> Vec<Self> {
        let mut themes: Vec<Self> = BUNDLED_THEMES.iter()
            .filter_map(|content| Self::from_config_str(content))
            .collect();

        if let Some(user_dir) = dirs::home_dir().map(|h| h.join(".config").join("doris").join("themes")) {
            if let Ok(entries) = std::fs::read_dir(&user_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                        if let Some(theme) = Self::from_config(&path) {
                            match themes.iter_mut().find(|t| t.name == theme.name) {
                                Some(existing) => *existing = theme,
                                None => themes.push(theme),
                            }
                        }
                    }
                }
            }
        }

        themes.sort_by(|a, b| a.name.cmp(&b.name));
        themes
    }
}

/// Convert an RGB theme color to the nearest of the 16 basic ANSI colors,
/// for the "Truecolor"/"False tty" Options toggles -- previously these
/// just flipped a persisted config value with no rendering effect at all.
/// Named/basic colors (e.g. `Color::Yellow` used for focus highlights)
/// pass through unchanged since they're already safe on any terminal.
///
/// `allow_bright` controls whether the 8 "bright"/high-intensity ANSI
/// colors are candidates too: Truecolor=false still allows them (256-ish
/// color terminals almost always support the bright 8), while False
/// tty=true restricts to the base 8 (real Linux console / very limited
/// terminals typically only reliably support those).
pub fn degrade_color(color: Color, allow_bright: bool) -> Color {
    let (r, g, b) = match color {
        Color::Rgb(r, g, b) => (r, g, b),
        other => return other,
    };

    const BASIC: [(Color, (u8, u8, u8)); 8] = [
        (Color::Black, (0, 0, 0)),
        (Color::Red, (170, 0, 0)),
        (Color::Green, (0, 170, 0)),
        (Color::Yellow, (170, 85, 0)),
        (Color::Blue, (0, 0, 170)),
        (Color::Magenta, (170, 0, 170)),
        (Color::Cyan, (0, 170, 170)),
        (Color::Gray, (170, 170, 170)),
    ];
    const BRIGHT: [(Color, (u8, u8, u8)); 8] = [
        (Color::DarkGray, (85, 85, 85)),
        (Color::LightRed, (255, 85, 85)),
        (Color::LightGreen, (85, 255, 85)),
        (Color::LightYellow, (255, 255, 85)),
        (Color::LightBlue, (85, 85, 255)),
        (Color::LightMagenta, (255, 85, 255)),
        (Color::LightCyan, (85, 255, 255)),
        (Color::White, (255, 255, 255)),
    ];

    let mut best = BASIC[0].0;
    let mut best_dist = i32::MAX;
    let mut consider = |candidate: Color, (cr, cg, cb): (u8, u8, u8)| {
        let dr = cr as i32 - r as i32;
        let dg = cg as i32 - g as i32;
        let db = cb as i32 - b as i32;
        let dist = dr * dr + dg * dg + db * db;
        if dist < best_dist {
            best_dist = dist;
            best = candidate;
        }
    };
    for (c, rgb) in BASIC {
        consider(c, rgb);
    }
    if allow_bright {
        for (c, rgb) in BRIGHT {
            consider(c, rgb);
        }
    }
    best
}
