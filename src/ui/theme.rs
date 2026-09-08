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
        toml::from_str(&content).ok()
    }

    pub fn load_themes() -> Vec<Self> {
        let mut themes = Vec::new();
        let themes_dir = dirs::home_dir()
            .map(|h| h.join(".config").join("doris").join("themes"))
            .or_else(|| std::env::current_dir().ok().map(|c| c.join("themes")))
            .unwrap_or_default();

        if let Ok(entries) = std::fs::read_dir(&themes_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                    if let Some(theme) = Self::from_config(&path) {
                        themes.push(theme);
                    }
                }
            }
        }
        themes.sort_by(|a, b| a.name.cmp(&b.name));
        themes
    }
}
