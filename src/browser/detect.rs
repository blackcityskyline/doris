use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum BrowserKind {
    Chrome,
    Brave,
    Helium,
}

impl std::fmt::Display for BrowserKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrowserKind::Chrome => write!(f, "Chrome/Chromium"),
            BrowserKind::Brave => write!(f, "Brave"),
            BrowserKind::Helium => write!(f, "Helium"),
        }
    }
}

const BROWSER_BINARIES: &[(&str, &[&str])] = &[
    ("chrome", &["google-chrome", "google-chrome-stable", "chromium", "chromium-browser"]),
    ("brave", &["brave", "brave-browser"]),
    ("helium", &["helium-browser", "helium"]),
];

pub fn detect_browser(requested: Option<&str>) -> Result<(BrowserKind, PathBuf)> {
    if let Some(req) = requested {
        let key = req.to_lowercase();
        let entry = BROWSER_BINARIES.iter().find(|(k, _)| *k == key.as_str());
        if let Some((_, binaries)) = entry {
            for binary in binaries.iter() {
                if let Ok(path) = which::which(binary) {
                    let kind = match key.as_str() {
                        "brave" => BrowserKind::Brave,
                        "helium" => BrowserKind::Helium,
                        _ => BrowserKind::Chrome,
                    };
                    return Ok((kind, path));
                }
            }
            anyhow::bail!("Browser '{}' not found. Tried: {}", req, binaries.join(", "));
        }
        anyhow::bail!("Unknown browser '{}'. Supported: chrome, brave, helium", req);
    }

    for (key, binaries) in BROWSER_BINARIES {
        for binary in binaries.iter() {
            if let Ok(path) = which::which(binary) {
                let kind = match *key {
                    "brave" => BrowserKind::Brave,
                    "helium" => BrowserKind::Helium,
                    _ => BrowserKind::Chrome,
                };
                return Ok((kind, path));
            }
        }
    }

    anyhow::bail!("No supported browser found. Install one of: google-chrome, chromium, brave, helium-browser")
}
