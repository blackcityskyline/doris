use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::sources::source::KNOWN_SOURCES;
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
    /// Whether streaming goes through TorrServer at all. `false` means
    /// `spawn_stream` refuses with the reason instead of reaching for a
    /// server the user has switched off -- the app-side gate that replaces
    /// a guessed-at `systemctl` flow (which would need the user's sudo
    /// password and assume their deployment).
    #[serde(default = "default_true")]
    pub enable_torrserver: bool,
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
    /// Every source id this config has been shown to know -- the key
    /// that lets `migrate_sources` tell "new to this build" apart from
    /// "the user turned it off". Written on every save; empty only in a
    /// config written before B8 wave 1, which is exactly the case the
    /// migration has to read carefully.
    #[serde(default)]
    pub known_sources: Vec<String>,

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
            enable_torrserver: default_true(),
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
            known_sources: KNOWN_SOURCES.iter().map(|s| s.id.to_string()).collect(),
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
    // Rows via `,`, columns via `|`. The first one is the default UI:
    // Results across the top, Trackers and Log under it, no Torrent.
    vec![
        "1,3|4".to_string(),
        "1,2,3,4".to_string(),
        "1,3".to_string(),
        "1,2".to_string(),
    ]
}

fn default_enabled_sources() -> Vec<String> {
    // "Every implemented source ships turned on" -- stated against the
    // registry rather than as a second handwritten list, so a source
    // can only be left out of the defaults by not being implemented.
    KNOWN_SOURCES
        .iter()
        .filter(|s| s.implemented)
        .map(|s| s.id.to_string())
        .collect()
}

/// Source ids a config written before B8 wave 1 could possibly mention:
/// exactly what `KNOWN_SOURCES` held at `9d5ae14`, the last commit
/// before wave 1 added yts. Used only to seed `known_sources` for a
/// config that predates the field (see `migrate_sources`).
const LEGACY_SOURCES: &[&str] = &["rutracker", "rutor", "nnmclub"];

fn default_download_dir_mode() -> String {
    "default".to_string()
}

fn default_config_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    home.join(".config").join("doris").join("config.toml")
}

impl Config {
    /// Give a config the source ids it has never heard of.
    ///
    /// `enabled_sources` is opt-in, so a config saved before wave 1 --
    /// which lists only the sources that existed then -- would keep
    /// yts/tpb/subsplease/eztv switched off forever, with no UI able to
    /// switch them on until this build's Options rows arrived. That is
    /// not a hypothetical: it is what a live run of the finished wave
    /// hit.
    ///
    /// The line between "new to this build" and "the user turned it
    /// off" is [`Config::known_sources`]: an id this config has already
    /// seen is never re-added. A config predating the field is
    /// recognised by being empty and seeded with [`LEGACY_SOURCES`],
    /// which is what keeps somebody who disabled `rutor` back then from
    /// having it silently switched back on, while `tpb` -- an id they
    /// have never seen -- arrives enabled.
    ///
    /// "Seen" means *had a chance to be decided*, and a planned source
    /// gives no chance: its Options row is a caption, not a toggle, so
    /// an id the registry listed while it was still unbuilt was never
    /// something the user could accept or reject. Such ids are read as
    /// unknown and never written back, which is what makes the flip
    /// from planned to implemented arrive enabled -- wave 3's nnmclub
    /// was caught by exactly this hole (it sat in `known_sources` as a
    /// placeholder, then went live and stayed switched off), and its
    /// two followers in the same registry are what the rule now covers.
    pub fn migrate_sources(&mut self) {
        let seen: Vec<String> = if self.known_sources.is_empty() {
            LEGACY_SOURCES.iter().map(|s| s.to_string()).collect()
        } else {
            self.known_sources.clone()
        };
        // Drop the ids the registry lists but has not built: today
        // those rows cannot be toggled, so nothing was ever decided
        // about them. What is left -- implemented ids plus ids this
        // registry does not list at all -- is what counts as known.
        let known: Vec<String> = seen
            .iter()
            .filter(|id| {
                !KNOWN_SOURCES
                    .iter()
                    .any(|info| info.id == **id && !info.implemented)
            })
            .cloned()
            .collect();

        for id in default_enabled_sources() {
            let known_before = known.iter().any(|k| k == &id);
            let already_on = self.enabled_sources.iter().any(|e| e == &id);
            if !known_before && !already_on {
                self.enabled_sources.push(id);
            }
        }

        // Record every id this build knows *as something that could be
        // decided on* -- implemented ids only, by the same rule as
        // above, so a placeholder row never counts as the user having
        // seen it. A source added to the registry later is then unknown
        // again, which is what makes the *next* migration happen
        // without anyone extending a baseline; ids the registry no
        // longer lists are kept, since a config that knew them did not
        // stop knowing them.
        let mut all: Vec<String> = KNOWN_SOURCES
            .iter()
            .filter(|info| info.implemented)
            .map(|info| info.id.to_string())
            .collect();
        for id in known {
            if !all.iter().any(|a| a == &id) {
                all.push(id);
            }
        }
        self.known_sources = all;
    }
}

/// Parse config TOML and run the migrations it needs: the single entry
/// point `load` and the tests share, so what a test asserts is what a
/// real config file goes through.
pub fn from_toml(content: &str) -> Result<Config> {
    let mut config: Config = toml::from_str(content)?;
    config.migrate_sources();
    Ok(config)
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
            from_toml(&content)
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
