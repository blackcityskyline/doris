use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentItem {
    pub title: String,
    pub size: String,
    pub seeds: String,
    pub download_url: String,
    pub query: String,
    pub date: String,
    pub page_url: String,
}
