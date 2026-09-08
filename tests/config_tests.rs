use doris::config::*;

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
