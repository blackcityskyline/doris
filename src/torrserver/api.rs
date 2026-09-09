use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;

/// One torrent's live status, as reported by TorrServer's `/torrents`
/// endpoint (`{"action": "list"}` or `{"action": "get", "hash": ...}`).
///
/// Field names are `#[serde(rename)]`d to match TorrServer's Go JSON
/// output verbatim (capitalized, no json tags on the upstream struct) --
/// see https://github.com/YouROK/TorrServer server/torr/torrent.go and
/// server/web/api/utils. TorrServer has several community forks with
/// slightly different response shapes; every field here has
/// `#[serde(default)]` so an unfamiliar/renamed field degrades to a zero
/// value instead of failing to parse the whole list.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct TorrentInfo {
    #[serde(rename = "Name", default)]
    pub name: String,
    #[serde(rename = "Hash", default)]
    pub hash: String,
    #[serde(rename = "TorrentSize", default)]
    pub total_size: i64,
    #[serde(rename = "LoadedSize", default)]
    pub loaded_size: i64,
    #[serde(rename = "DownloadSpeed", default)]
    pub download_speed: f64,
    #[serde(rename = "UploadSpeed", default)]
    pub upload_speed: f64,
    #[serde(rename = "TotalPeers", default)]
    pub total_peers: i64,
    #[serde(rename = "ActivePeers", default)]
    pub active_peers: i64,
    #[serde(rename = "ConnectedSeeders", default)]
    pub connected_seeders: i64,
    #[serde(rename = "TorrentStatusString", default)]
    pub status_string: String,
}

impl TorrentInfo {
    /// Fraction downloaded, 0.0-1.0. 0.0 if the total size isn't known yet
    /// (torrent just added, metadata still loading).
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
        Ok(self.client
            .post(&url)
            .json(&body)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await?)
    }

    /// All torrents TorrServer currently knows about (`{"action": "list"}`).
    pub async fn list_torrents(&self) -> Result<Vec<TorrentInfo>> {
        let resp = self.torrents_action(serde_json::json!({ "action": "list" })).await?;
        // TorrServer returns `null` (not `[]`) when there are no torrents;
        // treat that the same as an empty list rather than an error.
        let list: Option<Vec<TorrentInfo>> = resp.json().await.unwrap_or(None);
        Ok(list.unwrap_or_default())
    }

    /// A single torrent's status (`{"action": "get", "hash": ...}`).
    pub async fn get_torrent(&self, hash: &str) -> Result<Option<TorrentInfo>> {
        let resp = self.torrents_action(serde_json::json!({ "action": "get", "hash": hash })).await?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        Ok(resp.json().await.ok())
    }

    /// Stop an active torrent's download/seeding without forgetting it.
    /// TorrServer has no dedicated "pause" action; `drop` is the standard
    /// way clients implement pause (it stops network activity but keeps
    /// the torrent's metadata, unlike `rem` which forgets it entirely).
    pub async fn pause(&self, hash: &str) -> Result<()> {
        self.torrents_action(serde_json::json!({ "action": "drop", "hash": hash })).await?;
        Ok(())
    }

    /// Resume a paused (dropped) torrent. There's no dedicated "resume"
    /// action either; re-`get`-ting a dropped torrent's hash makes
    /// TorrServer reload and resume it.
    pub async fn resume(&self, hash: &str) -> Result<()> {
        let _ = self.get_torrent(hash).await?;
        Ok(())
    }

    /// Remove a torrent entirely (`{"action": "rem", "hash": ...}`).
    pub async fn remove(&self, hash: &str) -> Result<()> {
        self.torrents_action(serde_json::json!({ "action": "rem", "hash": hash })).await?;
        Ok(())
    }

    pub async fn upload_torrent(&self, torrent_bytes: &[u8], title: &str) -> Result<String> {
        let safe_title = title
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-')
            .collect::<String>()
            .trim()
            .to_string();

        let filename = format!("{}.torrent", if safe_title.is_empty() { "torrent".to_string() } else { safe_title });

        let form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(torrent_bytes.to_vec())
                    .file_name(filename)
                    .mime_str("application/x-bittorrent")?,
            );

        let url = format!("{}/torrent/upload?save=db", self.base_url);
        let response = self.client
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

    pub async fn play(&self, hash: &str, title: &str, player: Option<&str>) -> Result<tokio::process::Child> {
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
        let child = tokio::process::Command::new(player_name)
            .arg(&stream_url)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to launch {}: {}", player_name, e))?;

        Ok(child)
    }
}
