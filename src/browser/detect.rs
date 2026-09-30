use anyhow::Result;
use std::path::PathBuf;

/// Every browser Doris knows how to drive. All four are Chromium-based and
/// speak the same CDP/WebDriver protocol (see `browser::cdp`).
///
/// Adding a fifth means appending one `BROWSER_ROWS` entry (key, extra
/// accepted keys, binary names, label) and one enum variant -- nothing else
/// in the browser layer changes. The four methods below read the row, so
/// the table is the only place a browser's details live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BrowserKind {
    Chrome,
    Chromium,
    Brave,
    Helium,
}

/// `(config key, extra accepted keys, binary names to probe, display label)`.
type BrowserRow = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    &'static str,
);

/// One row per [`BrowserKind`]: `(kind, (key, extra accepted keys, binaries
/// to probe, label))`. The single source of truth for `BrowserKind`'s
/// methods -- see the guard test in `tests/browser_detect_tests.rs`.
const BROWSER_ROWS: &[(BrowserKind, BrowserRow)] = &[
    (
        BrowserKind::Chrome,
        (
            "chrome",
            &["google-chrome"],
            &["google-chrome", "google-chrome-stable"],
            "Chrome",
        ),
    ),
    (
        BrowserKind::Chromium,
        (
            "chromium",
            &[],
            &["chromium", "chromium-browser"],
            "Chromium",
        ),
    ),
    (
        BrowserKind::Brave,
        ("brave", &[], &["brave", "brave-browser"], "Brave"),
    ),
    (
        BrowserKind::Helium,
        ("helium", &[], &["helium-browser", "helium"], "Helium"),
    ),
];

/// The table row for `self` (key, aliases, binaries, label).
///
/// Invariant: every variant appears exactly once in `BROWSER_ROWS` -- pinned
/// by the guard test in `tests/browser_detect_tests.rs`. The `unwrap_or`
/// fallback is unreachable in practice; it exists to satisfy the
/// "never `.unwrap()` in production" rule without changing the API.
fn row(kind: BrowserKind) -> BrowserRow {
    BROWSER_ROWS
        .iter()
        .find(|(k, ..)| *k == kind)
        .map(|&(_, row)| row)
        .unwrap_or(("", &[], &[], ""))
}

impl BrowserKind {
    /// Stable lowercase identifier used in config files and CLI flags.
    pub fn config_key(&self) -> &'static str {
        row(*self).0
    }

    pub fn from_config_key(key: &str) -> Option<Self> {
        let key = key.to_lowercase();
        BROWSER_ROWS
            .iter()
            .find(|(_, (main, aliases, ..))| *main == key || aliases.iter().any(|a| *a == key))
            .map(|(kind, ..)| *kind)
    }

    /// Binaries to probe with `which`, in preference order.
    pub fn binaries(&self) -> &'static [&'static str] {
        row(*self).2
    }

    /// Human-facing name (menus, health check).
    pub fn label(&self) -> &'static str {
        row(*self).3
    }
}

impl std::fmt::Display for BrowserKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
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
    let parsed: Vec<BrowserKind> = raw
        .iter()
        .filter_map(|s| BrowserKind::from_config_key(s))
        .collect();
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
        let kind = BrowserKind::from_config_key(req).ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown browser '{}'. Supported: chrome, chromium, brave, helium",
                req
            )
        })?;
        for binary in kind.binaries() {
            if let Ok(path) = which::which(binary) {
                return Ok((kind, path));
            }
        }
        anyhow::bail!(
            "Browser '{}' not found. Tried: {}",
            req,
            kind.binaries().join(", ")
        );
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
