use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
#[allow(dead_code)]
pub struct Config {
    pub browser: Option<String>,
    /// Browser window visibility: "visible" or "hidden". Defaults to hidden
    /// (background) so a first run never pops a browser window.
    #[serde(default = "default_browser_visibility", alias = "browser_mode")]
    pub browser_visibility: String,
    /// Order to probe installed browsers in when `browser` isn't set to a
    /// specific one. Any of "chrome", "chromium", "brave", "helium".
    #[serde(default = "default_browser_priority")]
    pub browser_priority: Vec<String>,
    #[serde(default = "default_torrserver_url")]
    pub torrserver_url: String,
    #[serde(default = "default_bridge_port")]
    pub bridge_port: u16,
    #[serde(default = "default_cookie_file")]
    pub cookie_file: String,
    pub keybindings: Option<Keybindings>,

    // --- Options / "general" category (ROADMAP.md Phase 5) -----------------
    // These mirror btop++'s general settings page. Values here are the
    // single source of truth the Options modal reads and writes -- unlike
    // the pre-Phase-5 UI, which displayed hardcoded literals with no
    // backing field at all.
    /// Name of the active theme file (without extension). `None` means
    /// "use the built-in default theme" (see `ui::theme::Theme::default_theme`).
    pub theme_name: Option<String>,
    #[serde(default = "default_true")]
    pub theme_background: bool,
    #[serde(default = "default_true")]
    pub truecolor: bool,
    #[serde(default)]
    pub false_tty: bool,
    #[serde(default = "default_true")]
    pub vim_keys: bool,
    #[serde(default)]
    pub disable_mouse: bool,
    #[serde(default)]
    pub disable_presets: bool,
    /// Each entry is a comma-separated list of zone key characters (the
    /// same digits used for the 1/2/3/4 zone-toggle keybinds) describing
    /// which zones a preset shows, e.g. "1,2,3,4" for every zone.
    #[serde(default = "default_presets")]
    pub presets: Vec<String>,
    #[serde(default)]
    pub preset_index: usize,
    #[serde(default = "default_true")]
    pub show_boxes: bool,
    /// Poll interval, in milliseconds, for the torrent status panel
    /// (consumed by `torrent::Manager`, ROADMAP.md Phase 7).
    #[serde(default = "default_update_ms")]
    pub update_ms: u64,
    #[serde(default = "default_true")]
    pub rounded_corners: bool,
    #[serde(default = "default_true")]
    pub terminal_sync: bool,
    /// Symbol set for graph/sparkline widgets (ROADMAP.md Phase 8's
    /// btop-style dot progress bar). One of "braille", "block", "dot".
    #[serde(default = "default_graph_symbol")]
    pub graph_symbol: String,
    #[serde(default)]
    pub save_config_on_exit: bool,

    // --- Options / "streaming" category (ROADMAP.md Phase 6) ---------------
    /// Kill the automated browser when Doris exits. Note: this is already
    /// the default outcome of `Browser`'s `Drop` impl regardless of this
    /// flag; setting this to `false` intentionally leaks the browser
    /// handle at exit so the browser process survives past Doris closing.
    #[serde(default = "default_true")]
    pub close_browser_on_exit: bool,
    #[serde(default = "default_true")]
    pub save_cookies: bool,
    #[serde(default = "default_true")]
    pub save_credentials: bool,
    /// Which entries in `search::source::KNOWN_SOURCES` are active. A
    /// source id not in this list is treated as disabled even if
    /// implemented.
    #[serde(default = "default_enabled_sources")]
    pub enabled_sources: Vec<String>,

    // --- Options / "download" category (ROADMAP.md Phase 6) ----------------
    #[serde(default = "default_true")]
    pub download_enabled: bool,
    /// "default" (OS Downloads folder) or "custom1"/"custom2"/"custom3"
    /// (one of the three slots below).
    #[serde(default = "default_download_dir_mode")]
    pub download_dir_mode: String,
    #[serde(default)]
    pub download_dir_custom_1: String,
    #[serde(default)]
    pub download_dir_custom_2: String,
    #[serde(default)]
    pub download_dir_custom_3: String,
    #[serde(default)]
    pub download_sequential: bool,
    /// 0 means unlimited. Not yet wired to TorrServer's API -- see
    /// ROADMAP.md Phase 7 (`torrent::Manager` is meant to own all
    /// TorrServer interaction instead of piecemeal additions to the thin
    /// client in `torrserver/api.rs`).
    #[serde(default)]
    pub download_speed_limit_kbps: u32,
    #[serde(default)]
    pub upload_speed_limit_kbps: u32,
    #[serde(default = "default_true")]
    pub close_torrent_core_on_exit: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            browser: None,
            browser_visibility: default_browser_visibility(),
            browser_priority: default_browser_priority(),
            torrserver_url: default_torrserver_url(),
            bridge_port: default_bridge_port(),
            cookie_file: default_cookie_file(),
            keybindings: None,
            theme_name: None,
            theme_background: true,
            truecolor: true,
            false_tty: false,
            vim_keys: true,
            disable_mouse: false,
            disable_presets: false,
            presets: default_presets(),
            preset_index: 0,
            show_boxes: true,
            update_ms: default_update_ms(),
            rounded_corners: true,
            terminal_sync: true,
            graph_symbol: default_graph_symbol(),
            save_config_on_exit: false,
            close_browser_on_exit: true,
            save_cookies: true,
            save_credentials: true,
            enabled_sources: default_enabled_sources(),
            download_enabled: true,
            download_dir_mode: default_download_dir_mode(),
            download_dir_custom_1: String::new(),
            download_dir_custom_2: String::new(),
            download_dir_custom_3: String::new(),
            download_sequential: false,
            download_speed_limit_kbps: 0,
            upload_speed_limit_kbps: 0,
            close_torrent_core_on_exit: true,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[allow(dead_code)]
pub struct Keybindings {
    pub quit: Option<String>,
    pub focus_search: Option<String>,
    pub cursor_up: Option<String>,
    pub cursor_down: Option<String>,
    pub select: Option<String>,
}

fn default_torrserver_url() -> String {
    "http://127.0.0.1:8090".to_string()
}

fn default_browser_visibility() -> String {
    "hidden".to_string()
}

fn default_browser_priority() -> Vec<String> {
    // Mirrors browser::detect::DEFAULT_PRIORITY. Kept as plain strings here
    // so config.rs doesn't need to depend on the browser module just for
    // this default; browser::detect::parse_priority() re-derives the
    // BrowserKind order from these strings and falls back to its own
    // DEFAULT_PRIORITY if the list is ever empty or unparsable.
    ["helium", "brave", "chrome", "chromium"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn default_bridge_port() -> u16 {
    14141
}

fn default_cookie_file() -> String {
    "cookies.txt".to_string()
}

fn default_true() -> bool {
    true
}

fn default_update_ms() -> u64 {
    1000
}

fn default_graph_symbol() -> String {
    "braille".to_string()
}

fn default_presets() -> Vec<String> {
    vec!["1,2,3,4".to_string(), "1,3".to_string(), "1,2".to_string()]
}

fn default_enabled_sources() -> Vec<String> {
    // Only sources that are actually implemented (see
    // search::source::KNOWN_SOURCES) are enabled by default.
    vec!["rutracker".to_string()]
}

fn default_download_dir_mode() -> String {
    "default".to_string()
}

fn default_config_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    home.join(".config").join("doris").join("config.toml")
}

pub fn load(path: Option<&Path>) -> Result<Config> {
    let config_path = match path {
        Some(p) => Some(p.to_path_buf()),
        None => {
            let candidate = default_config_path();
            if candidate.exists() {
                Some(candidate)
            } else {
                None
            }
        }
    };

    match config_path {
        Some(p) => {
            let content = std::fs::read_to_string(&p)?;
            let config: Config = toml::from_str(&content)?;
            Ok(config)
        }
        None => Ok(Config::default()),
    }
}

/// Write `config` back to disk as TOML, creating `~/.config/doris/` if it
/// doesn't exist yet. Used by "Save config on exit" (Options -> general)
/// and can be called directly for an explicit "save now" action later.
pub fn save(config: &Config, path: Option<&Path>) -> Result<()> {
    let config_path = match path {
        Some(p) => p.to_path_buf(),
        None => default_config_path(),
    };
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let toml_str = toml::to_string_pretty(config)?;
    std::fs::write(&config_path, toml_str)?;
    Ok(())
}
