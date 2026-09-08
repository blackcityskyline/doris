use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
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
        }
    }
}

#[derive(Debug, Deserialize, Default)]
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

pub fn load(path: Option<&Path>) -> Result<Config> {
    let config_path = match path {
        Some(p) => Some(p.to_path_buf()),
        None => {
            let home = dirs::home_dir().unwrap_or_default();
            let config_dir = home.join(".config").join("doris");
            let candidate = config_dir.join("config.toml");
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
