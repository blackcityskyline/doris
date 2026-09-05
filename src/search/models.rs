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
