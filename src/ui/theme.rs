use ratatui::style::{Color, Style};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

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
    pub menu_fg: ColorDef,
    pub menu_selected_bg: ColorDef,
    pub menu_selected_fg: ColorDef,
    /// Palette accents, all optional.
    pub primary: Option<ColorDef>,
    pub secondary: Option<ColorDef>,
    pub error: Option<ColorDef>,
    pub on_hover: Option<ColorDef>,
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

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

/// Every theme shipped in the repo's top-level `themes/` directory, embedded at compile time so
/// they always work regardless of the current working directory or install location.
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
            menu_fg: ColorDef::new(144, 164, 174),
            menu_selected_bg: ColorDef::new(20, 50, 80),
            menu_selected_fg: ColorDef::new(128, 222, 234),
            primary: None,
            secondary: None,
            error: None,
            on_hover: None,
        }
    }

    pub fn default_theme() -> Self {
        Self::dark()
    }

    /// The style of the row under the cursor in any list -- the results table, the Trackers
    /// panel, the detail modal's file list.
    pub fn selection_style(&self) -> Style {
        Style::default()
            .fg(self.selected_fg.to_color())
            .bg(self.selected_bg.to_color())
    }

    /// Structure accent: frame borders, zone/button words, modal and menu titles.
    pub fn primary_color(&self) -> Color {
        self.primary
            .as_ref()
            .map_or_else(|| self.title.to_color(), ColorDef::to_color)
    }

    /// Secondary accent: frame furniture that must stay distinguishable from the primary --
    /// zone numbers, table headers, row accents.
    pub fn secondary_color(&self) -> Color {
        self.secondary
            .as_ref()
            .map_or_else(|| self.hi_fg.to_color(), ColorDef::to_color)
    }

    /// Failure colour (source refusals, error log lines).
    pub fn error_color(&self) -> Color {
        self.error.as_ref().map_or(Color::Red, ColorDef::to_color)
    }

    /// The colour a keybind glyph is drawn in: what a hover would put on the accent, so the
    /// hotkey reads as the actionable part of the word.
    pub fn on_hover_color(&self) -> Color {
        self.on_hover
            .as_ref()
            .map_or_else(|| self.hi_fg.to_color(), ColorDef::to_color)
    }

    pub fn from_config(path: &std::path::Path) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        Self::from_config_str(&content)
    }

    fn from_config_str(content: &str) -> Option<Self> {
        toml::from_str(content).ok()
    }

    /// Every theme doris can offer: the bundled ones plus whatever the user has in
    /// `~/.config/doris/themes`.
    pub fn load_themes() -> Vec<Self> {
        static CACHE: OnceLock<Vec<Theme>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                let user_dir =
                    dirs::home_dir().map(|h| h.join(".config").join("doris").join("themes"));
                Self::load_themes_from(user_dir.as_deref())
            })
            .clone()
    }

    /// `load_themes()` with the user themes directory made explicit, so tests can point it at a
    /// temp dir instead of `$HOME`.
    pub fn load_themes_from(user_dir: Option<&std::path::Path>) -> Vec<Self> {
        let mut themes: Vec<Self> = BUNDLED_THEMES
            .iter()
            .filter_map(|content| Self::from_config_str(content))
            .collect();

        if let Some(user_dir) = user_dir {
            if let Ok(entries) = std::fs::read_dir(user_dir) {
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

/// Convert an RGB theme color to the nearest of the 16 basic ANSI colors, for the
/// "Truecolor"/"False tty" Options toggles -- previously these just flipped a persisted config
/// value with no rendering effect at all.
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
