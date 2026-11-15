use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub const DEFAULT_URL: &str = "http://127.0.0.1:8090";

/// What the server does with what it fetches: where it puts it, and whether it
/// is still there after the last reader leaves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoragePolicy {
    /// `TorrentsSavePath`: empty means the server's own default.
    pub path: String,
    /// `UseDisk`: cache to disk at all.
    pub use_disk: bool,
    /// `RemoveCacheOnDrop`: drop what was fetched 30 s after the last reader.
    pub remove_cache_on_drop: bool,
}

impl StoragePolicy {
    /// The same fact said the way a person asks it, which is how the CLI and
    /// the extension's checkbox both spell it.
    pub fn keep_cache(&self) -> bool {
        !self.remove_cache_on_drop
    }
}

/// One torrent's live status, as reported by TorrServer's `/torrents` endpoint (`{"action":
/// "list"}` or `{"action": "get", "hash":...}`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TorrentInfo {
    #[serde(rename = "Name", alias = "title", default)]
    pub name: String,
    #[serde(rename = "Hash", alias = "hash", default)]
    pub hash: String,
    #[serde(rename = "TorrentSize", alias = "torrent_size", default)]
    pub total_size: i64,
    #[serde(rename = "LoadedSize", alias = "loaded_size", default)]
    pub loaded_size: i64,
    #[serde(rename = "DownloadSpeed", alias = "download_speed", default)]
    pub download_speed: f64,
    #[serde(rename = "UploadSpeed", alias = "upload_speed", default)]
    pub upload_speed: f64,
    #[serde(rename = "TotalPeers", alias = "total_peers", default)]
    pub total_peers: i64,
    #[serde(rename = "ActivePeers", alias = "active_peers", default)]
    pub active_peers: i64,
    #[serde(rename = "ConnectedSeeders", alias = "connected_seeders", default)]
    pub connected_seeders: i64,
    #[serde(rename = "TorrentStatusString", alias = "stat_string", default)]
    pub status_string: String,
    /// A picture URL the server remembered when the torrent was added
    /// (`TorrentSpec.Poster`, often a TMDB or a tracker's own cover). Empty
    /// is the normal case: a magnet carries no artwork, and the panel's
    /// details then simply have no image to show.
    #[serde(rename = "Poster", alias = "poster", default)]
    pub poster: String,
    /// The server's category string, which is what its web UI groups by.
    #[serde(rename = "Category", alias = "category", default)]
    pub category: String,
    /// When the torrent was added, as a Unix timestamp.
    #[serde(rename = "Timestamp", alias = "timestamp", default)]
    pub timestamp: i64,
    /// TorrServer answers with no file list of its own; the names live in
    /// this field, as a JSON document inside a JSON document:
    /// `{"TorrServer":{"Files":[{"id":1,"path":"Movie/movie.mkv","length":N}]}}`.
    ///
    /// Parsed once on the way in rather than by every reader, because the
    /// alternative is a command that has to know how TorrServer packs its
    /// own metadata -- and a file list is exactly what "what is in this
    /// torrent" means.
    #[serde(default)]
    pub files: Vec<TorrentFile>,
    /// The blob `files` was parsed out of, kept so `list_torrents` can do
    /// the same for every row it did not already have.
    #[serde(rename = "data", default)]
    pub data: String,
}

impl TorrentInfo {
    /// The file list, read out of `data` if it was not filled in already.
    pub fn data_as_files(&self) -> Vec<TorrentFile> {
        parse_file_list(&self.data)
    }

    /// Whether the server has this torrent closed (what its panel calls
    /// paused). `drop` closes it and `get` opens it again; the status word
    /// is the only thing that says which of the two happened.
    pub fn is_paused(&self) -> bool {
        self.status_string.to_lowercase().contains("closed")
    }

    /// The file a "save this stream" means when the user has not picked
    /// one: the largest, which is the video in anything with a sample or a
    /// subtitle track next to it.
    pub fn largest_file(&self) -> Option<&TorrentFile> {
        self.files.iter().max_by_key(|file| file.length)
    }
}

/// Fill in the file list of every row the server sent.
fn unwrap(mut torrents: Vec<TorrentInfo>) -> Vec<TorrentInfo> {
    for torrent in &mut torrents {
        if torrent.files.is_empty() {
            torrent.files = torrent.data_as_files();
        }
    }
    torrents
}

/// One file inside a torrent, as TorrServer names it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TorrentFile {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub length: i64,
}

/// Where `data` keeps the file list, by the two keys it is nested under.
#[derive(Deserialize)]
struct FileListEnvelope {
    #[serde(rename = "TorrServer")]
    torrserver: FileList,
}

#[derive(Deserialize)]
struct FileList {
    #[serde(rename = "Files", default)]
    files: Vec<TorrentFile>,
}

/// Pull the file list out of the `data` blob, or an empty list when the
/// server sent nothing that looks like one.
pub fn parse_file_list(data: &str) -> Vec<TorrentFile> {
    let Ok(outer) = serde_json::from_str::<serde_json::Value>(data) else {
        return Vec::new();
    };
    outer
        .get("TorrServer")
        .and_then(|v| {
            serde_json::from_value::<FileListEnvelope>(serde_json::json!({
                "TorrServer": v
            }))
            .ok()
        })
        .map(|e| e.torrserver.files)
        .unwrap_or_default()
}

impl TorrentInfo {
    /// Fraction downloaded, 0.0-1.0.
    pub fn progress(&self) -> f64 {
        if self.total_size <= 0 {
            0.0
        } else {
            (self.loaded_size as f64 / self.total_size as f64).clamp(0.0, 1.0)
        }
    }
}

#[derive(Clone)]
pub struct TorrServer {
    client: Client,
    base_url: String,
}

impl TorrServer {
    pub fn new(base_url: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// The server this client talks to, for messages that have to name
    /// it (`TorrServer: reachable at http://...`).
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn is_reachable(&self) -> bool {
        self.client
            .get(&self.base_url)
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    async fn torrents_action(&self, body: serde_json::Value) -> Result<reqwest::Response> {
        let url = format!("{}/torrents", self.base_url);
        Ok(self
            .client
            .post(&url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await?)
    }
}

/// Reject a non-2xx answer from `/torrents` with a sentence the log can show.
async fn ensure_ok(resp: reqwest::Response, what: &str) -> Result<()> {
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    // TorrServer puts its reason in the body ("link is empty",...).
    let body = resp.text().await.unwrap_or_default();
    let body = body.trim();
    let reason = if body.is_empty() {
        String::new()
    } else {
        format!(" -- {}", body.chars().take(200).collect::<String>())
    };
    anyhow::bail!("TorrServer refused to {}: {}{}", what, status, reason);
}

impl TorrServer {
    pub async fn list_torrents(&self) -> Result<Vec<TorrentInfo>> {
        let resp = self
            .torrents_action(serde_json::json!({ "action": "list" }))
            .await?;
        // TorrServer returns `null` (not `[]`) when there are no torrents;
        let list: Option<Vec<TorrentInfo>> = resp.json().await.unwrap_or(None);
        Ok(unwrap(list.unwrap_or_default()))
    }

    pub async fn get_torrent(&self, hash: &str) -> Result<Option<TorrentInfo>> {
        let resp = self
            .torrents_action(serde_json::json!({ "action": "get", "hash": hash }))
            .await?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        let mut torrent: Option<TorrentInfo> = resp.json().await.ok();
        if let Some(t) = torrent.as_mut() {
            if t.files.is_empty() {
                t.files = t.data_as_files();
            }
        }
        Ok(torrent)
    }

    /// Read the server's own settings, which is where its download path
    /// and cache policy live.
    ///
    /// TorrServer has two generations of API and this one answers on both:
    /// `{"action":"get"}` on `/settings` for the old one, the same body on
    /// the new. The keys are capitalised because that is how the server
    /// spells them, so nothing has to translate them.
    pub async fn settings(&self) -> Result<serde_json::Value> {
        let url = format!("{}/settings", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "action": "get" }))
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await?;
        Ok(resp.json().await.unwrap_or(serde_json::Value::Null))
    }

    /// Write settings back, given the keys to change.
    pub async fn set_settings(&self, sets: serde_json::Value) -> Result<()> {
        let url = format!("{}/settings", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "action": "set", "sets": sets }))
            // Measured: one write plus its read-back takes ~3.5 s here, and a
            // 5 s ceiling refused a write that did apply.
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await?;
        ensure_ok(resp, "change the server settings").await
    }

    /// Where the server puts what it fetches, and whether it keeps it.
    pub async fn storage(&self) -> Result<StoragePolicy> {
        let settings = self.settings().await?;
        let path = settings
            .get("TorrentsSavePath")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let use_disk = settings
            .get("UseDisk")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        // TorrServer drops what it fetched 30 s after the last reader leaves
        // unless this is turned off; an absent key means the default, which is
        // to drop it.
        let remove_cache_on_drop = settings
            .get("RemoveCacheOnDrop")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        Ok(StoragePolicy {
            path,
            use_disk,
            remove_cache_on_drop,
        })
    }

    /// Read a torrent until it is finished, `stop` says otherwise, or
    /// `deadline` runs out. Returns the last thing it reported.
    pub async fn watch(
        &self,
        hash: &str,
        deadline: std::time::Duration,
        mut stop: impl FnMut(&TorrentInfo) -> bool,
    ) -> Result<Option<TorrentInfo>> {
        let give_up = tokio::time::Instant::now() + deadline;
        let mut last: Option<TorrentInfo> = None;
        loop {
            let Some(info) = self.get_torrent(hash).await? else {
                return Ok(last);
            };
            let finished = info.progress() >= 1.0;
            last = Some(info);
            if finished || stop(last.as_ref().expect("just stored")) {
                return Ok(last);
            }
            let Some(remaining) = give_up.checked_duration_since(tokio::time::Instant::now())
            else {
                return Ok(last);
            };
            tokio::time::sleep(remaining.min(std::time::Duration::from_secs(2))).await;
        }
    }

    /// Stop an active torrent's download/seeding without forgetting it.
    pub async fn pause(&self, hash: &str) -> Result<()> {
        let body = serde_json::json!({ "action": "drop", "hash": hash });
        let resp = self.torrents_action(body).await?;
        ensure_ok(resp, "pause the torrent").await
    }

    pub async fn resume(&self, hash: &str) -> Result<()> {
        let body = serde_json::json!({ "action": "get", "hash": hash });
        let resp = self.torrents_action(body).await?;
        ensure_ok(resp, "resume the torrent").await
    }

    /// Remove a torrent entirely (`{"action": "rem", "hash":...}`).
    pub async fn remove(&self, hash: &str) -> Result<()> {
        let body = serde_json::json!({ "action": "rem", "hash": hash });
        let resp = self.torrents_action(body).await?;
        ensure_ok(resp, "remove the torrent").await
    }

    /// The m3u playlist of one torrent, exactly the bytes its web UI's
    /// "playlist" button downloads: one `#EXTINF` + `/stream` URL per file.
    ///
    /// A playlist is how a whole season is handed to a player at once, and
    /// it is also the only API answer that can be kept on disk without the
    /// server running.
    pub async fn playlist(&self, hash: &str) -> Result<String> {
        let url = format!(
            "{}/playlist?hash={}",
            self.base_url,
            urlencoding::encode(hash)
        );
        let resp = self
            .client
            .get(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("TorrServer refused the playlist: {status} {}", body.trim());
        }
        Ok(resp.text().await?)
    }

    /// Fetch a poster the server holds a URL for. It is an ordinary link
    /// (`TorrentSpec.Poster`), often on another host, so this is a plain
    /// GET and a size cap: an oversized answer is a broken link or an HTML
    /// error page, not artwork.
    pub async fn fetch_poster(&self, url: &str) -> Result<Vec<u8>> {
        const MAX_POSTER: usize = 16 * 1024 * 1024;
        let resp = self
            .client
            .get(url)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("poster answered {status}");
        }
        let bytes = resp.bytes().await?;
        if bytes.len() > MAX_POSTER {
            anyhow::bail!("poster is {} bytes, too large", bytes.len());
        }
        Ok(bytes.to_vec())
    }

    /// Hand TorrServer a magnet link instead of a `.torrent` file: no download round trip, and
    /// the fetch starts from the DHT plus whatever trackers the link carries.
    pub async fn add_by_link(&self, link: &str, title: &str) -> Result<String> {
        let body = serde_json::json!({
            "action": "add",
            "link": link,
            "title": title,
            "save_to_db": true,
        });
        let response = self.torrents_action(body).await?;
        if !response.status().is_success() {
            // TorrServer reports why in the body ("link is empty",
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("TorrServer rejected the link: {}", text);
        }
        let json: serde_json::Value = response.json().await?;
        let hash = json
            .get("hash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("No hash returned from TorrServer"))?;
        Ok(hash.to_string())
    }

    pub async fn upload_torrent(&self, torrent_bytes: &[u8], title: &str) -> Result<String> {
        let safe_title = title
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-')
            .collect::<String>()
            .trim()
            .to_string();

        let filename = format!(
            "{}.torrent",
            if safe_title.is_empty() {
                "torrent".to_string()
            } else {
                safe_title
            }
        );

        let form = reqwest::multipart::Form::new().part(
            "file",
            reqwest::multipart::Part::bytes(torrent_bytes.to_vec())
                .file_name(filename)
                .mime_str("application/x-bittorrent")?,
        );

        let url = format!("{}/torrent/upload?save=db", self.base_url);
        let response = self
            .client
            .post(&url)
            .multipart(form)
            .header("title", title)
            .send()
            .await?;

        let json: serde_json::Value = response.json().await?;
        let hash = json
            .get("hash")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("No hash returned from TorrServer"))?;

        Ok(hash.to_string())
    }

    pub async fn play(
        &self,
        hash: &str,
        title: &str,
        player: Option<&str>,
    ) -> Result<tokio::process::Child> {
        let safe_title = title
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-')
            .collect::<String>()
            .trim()
            .to_string();

        let encoded_title = urlencoding::encode(&safe_title);
        let stream_url = format!(
            "{}/stream/{}.m3u?link={}&m3u&save=db",
            self.base_url, encoded_title, hash
        );

        let player_name = player.unwrap_or("mpv");
        let mut cmd = tokio::process::Command::new(player_name);
        // stdout is not read by anyone: `spawn_stream` only follows stderr
        // for the messages worth logging. A piped stdout nobody drains
        // fills its pipe buffer and blocks the player mid-stream, which is
        // exactly what mpv then looks like -- stuttering on a source that
        // plays fine when started by hand. A player writes nothing useful
        // to stdout anyway, so it goes to the terminal's null device.
        cmd.arg(&stream_url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());

        // Start the player in its own process group so it survives doris
        #[cfg(unix)]
        cmd.process_group(0);

        let child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to launch {}: {}", player_name, e))?;

        Ok(child)
    }

    /// Write one file of a torrent to `dest`, straight from the stream the
    /// player would have read.
    ///
    /// `index` is what `/stream` calls the file index: the id out of the
    /// file list, except for a one-file torrent, where the server ignores
    /// whatever is passed and always means that file (`index=1`).
    ///
    /// The response is copied chunk by chunk, so a film is never held in
    /// memory, and it lands under a temporary name first: a save cut off
    /// half way leaves a `.part` rather than a file that looks finished.
    pub async fn download_file(
        &self,
        hash: &str,
        index: u64,
        name: &str,
        dest: &std::path::Path,
    ) -> Result<u64> {
        let base = std::path::Path::new(name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("stream");
        let url = format!(
            "{}/stream/{}?link={}&index={}&play",
            self.base_url,
            urlencoding::encode(base),
            urlencoding::encode(hash),
            index
        );
        let mut response = self.client.get(&url).send().await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let body = body.trim();
            anyhow::bail!("TorrServer refused the download: {status} {body}");
        }

        let tmp = dest.with_file_name(format!(
            ".{}.part",
            dest.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("stream")
        ));
        let copied = async {
            use tokio::io::AsyncWriteExt;
            let mut out = tokio::fs::File::create(&tmp).await?;
            let mut written = 0u64;
            while let Some(chunk) = response.chunk().await? {
                out.write_all(&chunk).await?;
                written += chunk.len() as u64;
            }
            out.flush().await?;
            Ok::<u64, anyhow::Error>(written)
        }
        .await;
        match copied {
            Ok(written) => {
                tokio::fs::rename(&tmp, dest).await?;
                Ok(written)
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                Err(e)
            }
        }
    }
}
