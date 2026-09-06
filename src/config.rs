use anyhow::Result;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Config {
    pub browser: Option<String>,
    #[serde(default = "default_browser_mode")]
    pub browser_mode: String,
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
            browser_mode: default_browser_mode(),
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

fn default_browser_mode() -> String {
    "gui".to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.browser_mode, "gui");
        assert_eq!(config.torrserver_url, "http://127.0.0.1:8090");
        assert_eq!(config.bridge_port, 14141);
        assert_eq!(config.cookie_file, "cookies.txt");
        assert!(config.browser.is_none());
    }

    #[test]
    fn test_config_parse_toml() {
        let toml_str = r#"
            browser = "helium"
            browser_mode = "headless"
            torrserver_url = "http://192.168.1.100:8090"
            bridge_port = 14142
            cookie_file = "/tmp/cookies.txt"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.browser.as_deref(), Some("helium"));
        assert_eq!(config.browser_mode, "headless");
        assert_eq!(config.torrserver_url, "http://192.168.1.100:8090");
        assert_eq!(config.bridge_port, 14142);
        assert_eq!(config.cookie_file, "/tmp/cookies.txt");
    }

    #[test]
    fn test_config_parse_partial_toml() {
        let toml_str = r#"
            browser = "brave"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.browser.as_deref(), Some("brave"));
        assert_eq!(config.browser_mode, "gui");
        assert_eq!(config.torrserver_url, "http://127.0.0.1:8090");
    }

    #[test]
    fn test_config_parse_empty() {
        let config: Config = toml::from_str("").unwrap();
        assert!(config.browser.is_none());
        assert_eq!(config.browser_mode, "gui");
    }

    #[test]
    fn test_config_parse_with_keybindings() {
        let toml_str = r#"
            [keybindings]
            quit = "ctrl+x"
            focus_search = "/"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let kb = config.keybindings.unwrap();
        assert_eq!(kb.quit.as_deref(), Some("ctrl+x"));
        assert_eq!(kb.focus_search.as_deref(), Some("/"));
    }
}
