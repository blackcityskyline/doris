use anyhow::Result;
use std::path::PathBuf;

/// Every browser Doris knows how to drive. All four are Chromium-based and
/// speak the same CDP/WebDriver protocol (see `browser::cdp`), so adding a
/// fifth Chromium-family browser is just a new arm here plus a new entry in
/// `BROWSER_BINARIES` — nothing else in the browser layer needs to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowserKind {
    Chrome,
    Chromium,
    Brave,
    Helium,
}

impl BrowserKind {
    /// Stable lowercase identifier used in config files and CLI flags.
    pub fn config_key(&self) -> &'static str {
        match self {
            BrowserKind::Chrome => "chrome",
            BrowserKind::Chromium => "chromium",
            BrowserKind::Brave => "brave",
            BrowserKind::Helium => "helium",
        }
    }

    pub fn from_config_key(key: &str) -> Option<Self> {
        match key.to_lowercase().as_str() {
            "chrome" | "google-chrome" => Some(BrowserKind::Chrome),
            "chromium" => Some(BrowserKind::Chromium),
            "brave" => Some(BrowserKind::Brave),
            "helium" => Some(BrowserKind::Helium),
            _ => None,
        }
    }

    fn binaries(&self) -> &'static [&'static str] {
        match self {
            BrowserKind::Helium => &["helium-browser", "helium"],
            BrowserKind::Brave => &["brave", "brave-browser"],
            BrowserKind::Chrome => &["google-chrome", "google-chrome-stable"],
            BrowserKind::Chromium => &["chromium", "chromium-browser"],
        }
    }
}

impl std::fmt::Display for BrowserKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrowserKind::Chrome => write!(f, "Chrome"),
            BrowserKind::Chromium => write!(f, "Chromium"),
            BrowserKind::Brave => write!(f, "Brave"),
            BrowserKind::Helium => write!(f, "Helium"),
        }
    }
}

/// Order browsers are probed in when none is explicitly requested. This is
/// the fallback used by [`detect_browser`]; callers that know the user's
/// configured "Prioritize browser" order should call
/// [`detect_browser_with_priority`] instead.
pub const DEFAULT_PRIORITY: &[BrowserKind] = &[
    BrowserKind::Helium,
    BrowserKind::Brave,
    BrowserKind::Chrome,
    BrowserKind::Chromium,
];

/// Parse a user-supplied priority list (e.g. from Options or `config.toml`'s
/// `browser_priority = ["brave", "chrome"]`) into `BrowserKind`s, silently
/// dropping unknown entries. Falls back to [`DEFAULT_PRIORITY`] if the
/// result would otherwise be empty, so a typo'd config can never leave the
/// app with no browsers to try.
pub fn parse_priority(raw: &[String]) -> Vec<BrowserKind> {
    let parsed: Vec<BrowserKind> = raw.iter().filter_map(|s| BrowserKind::from_config_key(s)).collect();
    if parsed.is_empty() {
        DEFAULT_PRIORITY.to_vec()
    } else {
        parsed
    }
}

/// Detect an installed browser, trying [`DEFAULT_PRIORITY`] order when
/// `requested` is `None`. Convenience wrapper around
/// [`detect_browser_with_priority`] for callers that don't have a
/// user-configured priority list on hand (e.g. one-off health checks).
pub fn detect_browser(requested: Option<&str>) -> Result<(BrowserKind, PathBuf)> {
    detect_browser_with_priority(requested, DEFAULT_PRIORITY)
}

pub fn detect_browser_with_priority(
    requested: Option<&str>,
    priority: &[BrowserKind],
) -> Result<(BrowserKind, PathBuf)> {
    if let Some(req) = requested {
        let kind = BrowserKind::from_config_key(req)
            .ok_or_else(|| anyhow::anyhow!("Unknown browser '{}'. Supported: chrome, chromium, brave, helium", req))?;
        for binary in kind.binaries() {
            if let Ok(path) = which::which(binary) {
                return Ok((kind, path));
            }
        }
        anyhow::bail!("Browser '{}' not found. Tried: {}", req, kind.binaries().join(", "));
    }

    for &kind in priority {
        for binary in kind.binaries() {
            if let Ok(path) = which::which(binary) {
                return Ok((kind, path));
            }
        }
    }

    anyhow::bail!("No supported browser found. Install one of: google-chrome, chromium, brave, helium-browser")
}
