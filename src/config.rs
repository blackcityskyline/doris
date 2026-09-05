use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    pub browser: Option<String>,
    #[serde(default = "default_torrserver_url")]
    pub torrserver_url: String,
    #[serde(default = "default_bridge_port")]
    pub bridge_port: u16,
    #[serde(default = "default_cookie_file")]
    pub cookie_file: String,
    pub keybindings: Option<Keybindings>,
}

#[derive(Debug, Deserialize, Default)]
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
            let config_dir = home.join(".config").join("t-hunter");
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
