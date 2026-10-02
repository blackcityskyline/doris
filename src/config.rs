use anyhow::Result;
use serde::{Deserialize, Serialize};

use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub browser: Option<String>,
    /// Browser window visibility: "visible" or "hidden".
    #[serde(default = "default_browser_visibility", alias = "browser_mode")]
    pub browser_visibility: String,
    /// Order to probe installed browsers in when `browser` isn't set to a specific one.
    #[serde(default = "default_browser_priority")]
    pub browser_priority: Vec<String>,
    #[serde(default = "default_torrserver_url")]
    pub torrserver_url: String,
    /// Whether streaming goes through TorrServer at all.
    #[serde(default = "default_true")]
    pub enable_torrserver: bool,
    #[serde(default = "default_bridge_port")]
    pub bridge_port: u16,
    #[serde(default = "default_cookie_file")]
    pub cookie_file: String,

    // --- Options / "general" category -----------------
    /// Name of the active theme file (without extension).
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
    /// Each entry is a comma-separated list of zone key characters (the same digits used for
    /// the 1/2/3/4 zone-toggle keybinds) describing which zones a preset shows, e.g.
    #[serde(default = "default_presets")]
    pub presets: Vec<String>,
    #[serde(default)]
    pub preset_index: usize,
    #[serde(default = "default_true")]
    pub show_boxes: bool,
    /// Poll interval, in milliseconds, for the torrent status panel
    /// (consumed by `torrent::Manager`).
    #[serde(default = "default_update_ms")]
    pub update_ms: u64,
    #[serde(default = "default_true")]
    pub rounded_corners: bool,
    #[serde(default = "default_true")]
    pub terminal_sync: bool,
    /// Symbol set for graph/sparkline widgets (the dot progress bar).
    #[serde(default = "default_graph_symbol")]
    pub graph_symbol: String,
    #[serde(default)]
    pub save_config_on_exit: bool,
    // --- Options / "welcome" category ------------------
    /// Play the greeting animation before the UI comes up.
    #[serde(default = "default_true")]
    pub welcome_enabled: bool,
    /// Name of the animation: a built-in, or a file in
    /// `~/.config/doris/welcome/`. An unknown name falls back to the
    /// first animation there is, so a typo cannot turn the greeting off.
    #[serde(default = "default_welcome_template")]
    pub welcome_template: String,
    /// Milliseconds between frames -- the speed.
    #[serde(default = "default_welcome_frame_ms")]
    pub welcome_frame_ms: u64,
    /// How long the animation plays in total, in milliseconds. Zero
    /// plays it once through, however long that takes.
    #[serde(default = "default_welcome_duration_ms")]
    pub welcome_duration_ms: u64,
    /// The greeting, put wherever the template wrote `{text}`.
    #[serde(default = "default_welcome_text")]
    pub welcome_text: String,

    /// Every source id this config has been shown to know -- the key that lets
    /// `sources::source::migrate_config` tell "new to this build" apart from "the user turned
    /// it off".
    #[serde(default)]
    pub known_sources: Vec<String>,

    // --- Options / "streaming" category ---------------
    /// Kill the automated browser when Doris exits.
    #[serde(default = "default_true")]
    pub close_browser_on_exit: bool,
    #[serde(default = "default_true")]
    pub save_cookies: bool,
    #[serde(default = "default_true")]
    pub save_credentials: bool,
    /// Which registered source ids are active.
    #[serde(default)]
    pub enabled_sources: Vec<String>,

    // --- Options / "download" category ----------------
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
    /// Stop the download when doris exits, instead of leaving it running on TorrServer.
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
            welcome_enabled: default_true(),
            welcome_template: default_welcome_template(),
            welcome_frame_ms: default_welcome_frame_ms(),
            welcome_duration_ms: default_welcome_duration_ms(),
            welcome_text: default_welcome_text(),
            // Filled by `sources::source::migrate_config`, which knows the
            known_sources: Vec::new(),
            close_browser_on_exit: true,
            save_cookies: true,
            save_credentials: true,
            enabled_sources: Vec::new(),
            download_enabled: true,
            download_dir_mode: default_download_dir_mode(),
            download_dir_custom_1: String::new(),
            download_dir_custom_2: String::new(),
            download_dir_custom_3: String::new(),
            close_torrent_core_on_exit: true,
        }
    }
}

fn default_torrserver_url() -> String {
    crate::torrserver::api::DEFAULT_URL.to_string()
}

fn default_browser_visibility() -> String {
    "hidden".to_string()
}

fn default_browser_priority() -> Vec<String> {
    // Mirrors browser::detect::DEFAULT_PRIORITY. Kept as plain strings here
    ["helium", "brave", "chrome", "chromium"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn default_bridge_port() -> u16 {
    14141
}

/// Where a saved rutracker session lives by default: beside the config, in `~/.config/doris/`.
fn default_cookie_file() -> String {
    let home = dirs::home_dir().unwrap_or_default();
    home.join(".config")
        .join("doris")
        .join("cookies.txt")
        .to_string_lossy()
        .into_owned()
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
    vec![
        "1,3|4".to_string(),
        "1,2,3,4".to_string(),
        "1,3".to_string(),
        "1,2".to_string(),
    ]
}

fn default_welcome_template() -> String {
    "doris".to_string()
}

fn default_welcome_frame_ms() -> u64 {
    90
}

fn default_welcome_duration_ms() -> u64 {
    1600
}

fn default_welcome_text() -> String {
    "Welcome to Doris".to_string()
}

fn default_download_dir_mode() -> String {
    "default".to_string()
}

fn default_config_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    home.join(".config").join("doris").join("config.toml")
}

/// The panel arrangement, in a file of its own beside the config.
///
/// It is session state rather than a preference, which is why it is
/// written on every exit instead of only when "Save config on exit" is
/// on: that option is about the settings, and losing the window you
/// arranged is not a setting anybody should have to opt back into.
///
/// Beside the config rather than inside it, so `--config some/file`
/// gets its own arrangement and a test running against a temporary
/// config is not reading the one the developer left at home.
fn layout_path(config_path: Option<&Path>) -> PathBuf {
    match config_path {
        Some(p) => p.with_file_name("layout.toml"),
        None => default_config_path().with_file_name("layout.toml"),
    }
}

/// Read the arrangement left by the last session. A missing or
/// unreadable file is not an error: it only means there is nothing to
/// restore, which is what a first run looks like.
pub fn load_layout(config_path: Option<&Path>) -> Option<crate::ui::layout::SavedLayout> {
    let content = std::fs::read_to_string(layout_path(config_path)).ok()?;
    toml::from_str(&content).ok()
}

pub fn save_layout(
    layout: &crate::ui::layout::SavedLayout,
    config_path: Option<&Path>,
) -> Result<()> {
    let path = layout_path(config_path);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, toml::to_string(layout)?)?;
    Ok(())
}

/// Parse config TOML and run the migrations it needs: the single entry
/// point `load` and the tests share, so what a test asserts is what a
/// real config file goes through.
pub fn from_toml(content: &str) -> Result<Config> {
    let mut config: Config = toml::from_str(content)?;
    crate::sources::source::migrate_config(&mut config);
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
            let mut config = from_toml(&content)?;
            if migrate_cookie_file(&mut config, &p) {
                // Persist the fix, or every run redoes the migration and
                if let Err(e) = save(&config, Some(&p)) {
                    crate::log::log("config", &format!("cookie path migration: {e}"));
                }
            }
            Ok(config)
        }
        // No config file is not an empty one. An empty *file* predates
        None => {
            let mut fresh = crate::sources::source::first_run();
            crate::sources::source::migrate_config(&mut fresh);
            Ok(fresh)
        }
    }
}

/// Resolve a relative `cookie_file` against the config's own directory and write the result
/// back.
fn migrate_cookie_file(config: &mut Config, config_path: &Path) -> bool {
    let configured = std::path::Path::new(&config.cookie_file);
    if configured.is_absolute() {
        return false;
    }

    let Some(dir) = config_path.parent() else {
        return false;
    };
    let target = dir.join(configured);

    // The old location, before anything below changes the config.
    let old = std::path::Path::new(configured);
    if old.is_file() && old != target {
        if let Err(e) = move_file(old, &target) {
            crate::log::log(
                "config",
                &format!(
                    "could not move {} to {}: {e}; re-login will be needed",
                    old.display(),
                    target.display()
                ),
            );
        }
    }

    config.cookie_file = target.to_string_lossy().into_owned();
    true
}

/// `rename` is atomic but cannot cross a filesystem boundary, and the two ends here routinely
/// are on different ones -- the config in `~/.config` on the root filesystem, the app started
/// from a mounted data disk or a tmpfs.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)
        }
    }
}

/// Write `config` back to disk as TOML, creating `~/.config/doris/` if it doesn't exist yet.
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
