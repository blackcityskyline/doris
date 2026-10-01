use anyhow::Result;
use serde::{Deserialize, Serialize};

use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize)]
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

    // --- Options / "general" category -----------------
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
    /// (consumed by `torrent::Manager`).
    #[serde(default = "default_update_ms")]
    pub update_ms: u64,
    #[serde(default = "default_true")]
    pub rounded_corners: bool,
    #[serde(default = "default_true")]
    pub terminal_sync: bool,
    /// Symbol set for graph/sparkline widgets (the btop-style dot
    /// progress bar). One of "braille", "block", "dot".
    #[serde(default = "default_graph_symbol")]
    pub graph_symbol: String,
    #[serde(default)]
    pub save_config_on_exit: bool,
    /// Every source id this config has been shown to know -- the key
    /// that lets `sources::source::migrate_config` tell "new to this build"
    /// apart from
    /// "the user turned it off". Written on every save; empty only in a
    /// config written before B8 wave 1, which is exactly the case the
    /// migration has to read carefully.
    #[serde(default)]
    pub known_sources: Vec<String>,

    // --- Options / "streaming" category ---------------
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
    /// Which registered source ids are active. A
    /// source id not in this list is treated as disabled even if
    /// implemented.
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
    /// Stop the download when doris exits, instead of leaving it running
    /// on TorrServer. Kept beside the download options because that is
    /// what it governs.
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
            // Filled by `sources::source::migrate_config`, which knows the
            // registry. `Config` is a settings file; the source list is a
            // fact about this build, and a fresh file gets it the same way
            // an old one does.
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

/// Where a saved rutracker session lives by default: beside the config,
/// in `~/.config/doris/`.
///
/// This used to be the bare relative string `"cookies.txt"`, which the
/// app resolved against whatever directory it was started in -- so
/// `target/release/doris` kept its session inside `target/release/`,
/// where the next `cargo clean` was the only thing that ever removed it.
/// A path that moves with the CWD is not a location.
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
    // Results across the top, Trackers and Log under it, no Torrent.
    vec![
        "1,3|4".to_string(),
        "1,2,3,4".to_string(),
        "1,3".to_string(),
        "1,2".to_string(),
    ]
}

fn default_download_dir_mode() -> String {
    "default".to_string()
}

fn default_config_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    home.join(".config").join("doris").join("config.toml")
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
                // the config on disk still says the old relative path.
                if let Err(e) = save(&config, Some(&p)) {
                    crate::log::log("config", &format!("cookie path migration: {e}"));
                }
            }
            Ok(config)
        }
        // No config file is not an empty one. An empty *file* predates
        // `known_sources`: the migration seeds it with the ids that
        // existed then and reads them as decided, so those three stay off.
        // A machine that never had one has decided nothing, and gets
        // everything this build implements.
        None => {
            let mut fresh = crate::sources::source::first_run();
            crate::sources::source::migrate_config(&mut fresh);
            Ok(fresh)
        }
    }
}

/// Resolve a relative `cookie_file` against the config's own directory and
/// write the result back. Returns whether anything changed.
///
/// Only a *relative* value is touched: an absolute one named a file on
/// purpose. A relative one named a different file depending on the shell's
/// working directory, so pinning it beside the config is the only reading
/// that keeps working. A file at the old location is moved rather than
/// abandoned, so the migration does not cost the session it holds; if the
/// move fails the path is still pinned, because writing into whatever
/// directory the app happens to be run from is the worse failure.
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

/// `rename` is atomic but cannot cross a filesystem boundary, and the two
/// ends here routinely are on different ones -- the config in `~/.config`
/// on the root filesystem, the app started from a mounted data disk or a
/// tmpfs. Fall back to copy-then-delete, and only delete once the copy is
/// on disk, so a failure anywhere leaves the session where it was instead
/// of removing it.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)
        }
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
