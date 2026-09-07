use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentItem {
    pub title: String,
    #[serde(default)]
    pub size: String,
    #[serde(default)]
    pub seeds: String,
    #[serde(default)]
    pub download_url: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub page_url: String,
}

pub fn resolve_url(url: &str) -> String {
    if url.starts_with("http") {
        url.to_string()
    } else if url.starts_with('/') {
        format!("https://rutracker.org{}", url)
    } else {
        format!("https://rutracker.org/forum/{}", url)
    }
}
