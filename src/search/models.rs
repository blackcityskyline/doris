use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// Which Source produced this result ("rutracker"/"rutor"/...),
    /// matching `search::source::Source::id()`. Needed once search can
    /// mix results from multiple sources at once ("all" tab) so
    /// downloading/streaming a given row knows which client to use.
    /// #[serde(default)] so Rutracker's existing JS-eval-produced JSON
    /// (which doesn't set this field) still deserializes fine; app.rs
    /// fills it in to "rutracker" right after deserializing there instead.
    #[serde(default)]
    pub source: String,
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
