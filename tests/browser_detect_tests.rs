use doris::browser::detect::{BrowserKind, DEFAULT_PRIORITY, detect_browser_with_priority, parse_priority};

#[test]
fn test_config_key_and_from_config_key_round_trip() {
    for kind in [BrowserKind::Chrome, BrowserKind::Chromium, BrowserKind::Brave, BrowserKind::Helium] {
        let key = kind.config_key();
        assert_eq!(BrowserKind::from_config_key(key), Some(kind));
    }
}

#[test]
fn test_from_config_key_is_case_insensitive() {
    assert_eq!(BrowserKind::from_config_key("CHROME"), Some(BrowserKind::Chrome));
    assert_eq!(BrowserKind::from_config_key("Brave"), Some(BrowserKind::Brave));
    assert_eq!(BrowserKind::from_config_key("HELIUM"), Some(BrowserKind::Helium));
}

#[test]
fn test_from_config_key_accepts_google_chrome_alias() {
    assert_eq!(BrowserKind::from_config_key("google-chrome"), Some(BrowserKind::Chrome));
}

#[test]
fn test_from_config_key_rejects_unknown() {
    assert_eq!(BrowserKind::from_config_key("firefox"), None);
    assert_eq!(BrowserKind::from_config_key(""), None);
    assert_eq!(BrowserKind::from_config_key("safari"), None);
}

#[test]
fn test_display_uses_proper_case_names() {
    assert_eq!(BrowserKind::Chrome.to_string(), "Chrome");
    assert_eq!(BrowserKind::Chromium.to_string(), "Chromium");
    assert_eq!(BrowserKind::Brave.to_string(), "Brave");
    assert_eq!(BrowserKind::Helium.to_string(), "Helium");
}

#[test]
fn test_chrome_and_chromium_are_distinct_kinds() {
    // Regression guard: these two used to be folded into a single
    // "Chrome/Chromium" variant before ROADMAP.md Phase 2.
    assert_ne!(BrowserKind::Chrome, BrowserKind::Chromium);
    assert_ne!(BrowserKind::Chrome.config_key(), BrowserKind::Chromium.config_key());
}

#[test]
fn test_parse_priority_preserves_order() {
    let raw = vec!["brave".to_string(), "chrome".to_string()];
    let parsed = parse_priority(&raw);
    assert_eq!(parsed, vec![BrowserKind::Brave, BrowserKind::Chrome]);
}

#[test]
fn test_parse_priority_drops_unknown_entries() {
    let raw = vec!["brave".to_string(), "firefox".to_string(), "helium".to_string()];
    let parsed = parse_priority(&raw);
    assert_eq!(parsed, vec![BrowserKind::Brave, BrowserKind::Helium]);
}

#[test]
fn test_parse_priority_falls_back_to_default_when_empty() {
    let parsed = parse_priority(&[]);
    assert_eq!(parsed, DEFAULT_PRIORITY.to_vec());
}

#[test]
fn test_parse_priority_falls_back_to_default_when_all_unknown() {
    let raw = vec!["firefox".to_string(), "safari".to_string()];
    let parsed = parse_priority(&raw);
    assert_eq!(parsed, DEFAULT_PRIORITY.to_vec());
}

#[test]
fn test_default_priority_contains_all_four_kinds_exactly_once() {
    let mut sorted = DEFAULT_PRIORITY.to_vec();
    sorted.sort_by_key(|k| k.config_key());
    let mut expected = vec![BrowserKind::Chrome, BrowserKind::Chromium, BrowserKind::Brave, BrowserKind::Helium];
    expected.sort_by_key(|k| k.config_key());
    assert_eq!(sorted, expected);
}

#[test]
fn test_detect_browser_with_unknown_requested_name_errors() {
    // "firefox" isn't a supported kind at all, regardless of what's
    // actually installed on the machine running this test.
    let result = detect_browser_with_priority(Some("firefox"), DEFAULT_PRIORITY);
    assert!(result.is_err());
}
