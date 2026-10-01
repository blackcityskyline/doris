use serde::{Deserialize, Serialize};

use super::format::parse_size;
use super::source::Group;

/// A number that arrives as either a JSON number or a numeric string.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum FlexNum {
    Number(i64),
    Text(String),
}

impl FlexNum {
    pub fn as_i64(&self) -> i64 {
        match self {
            FlexNum::Number(n) => *n,
            FlexNum::Text(s) => s.trim().parse::<i64>().unwrap_or(0),
        }
    }

    pub fn as_u64(&self) -> u64 {
        self.as_i64().max(0) as u64
    }

    pub fn as_u32(&self) -> u32 {
        self.as_i64().max(0) as u32
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
    /// Which Source produced this result ("rutracker"/"rutor"/...), matching
    /// `search::source::Source::id()`.
    #[serde(default)]
    pub source: String,

    // --- B1 fields (numeric/hash twins of the display strings above) ---
    // All `#[serde(default)]`: Rutracker's JS-eval-produced JSON only sets
    // the display fields, and any source that cannot provide one of these
    // leaves it at the zero value rather than failing to deserialize.
    /// Content group this row belongs to, or `None` when the result cannot be attributed to one
    /// (searched with "all categories", or a source that doesn't filter server-side).
    #[serde(default)]
    pub group: Option<Group>,
    /// Lower-case hex info hash, `""` when the source can't provide it
    /// (rutracker's rows carry no magnet link at all).
    #[serde(default)]
    pub info_hash: String,
    /// The row's magnet URI as-is, when it has one.
    #[serde(default)]
    pub magnet: Option<String>,
    /// Numeric twin of `size`, in bytes; `0` = unknown.
    #[serde(default)]
    pub size_bytes: u64,
    /// Numeric twin of `seeds`; `0` = unknown/none.
    #[serde(default)]
    pub seeds_n: u32,
    /// Peer/leech count (rutor parses one and used to throw it away).
    #[serde(default)]
    pub leechers: u32,
    /// Unix seconds when the row was added; `0` = unknown.
    #[serde(default)]
    pub added: i64,
}

/// One file inside a torrent, as the detail modal lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub size: String,
}

impl TorrentItem {
    /// Derive the numeric twins from the display strings a source produced.
    pub fn fill_from_display(&mut self) {
        self.size_bytes = parse_size(&self.size);
        self.seeds_n = self.seeds.trim().parse::<u32>().unwrap_or(0);
    }
}
