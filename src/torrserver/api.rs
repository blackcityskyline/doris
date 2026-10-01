use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;

pub const DEFAULT_URL: &str = "http://127.0.0.1:8090";

/// One torrent's live status, as reported by TorrServer's `/torrents` endpoint (`{"action":
/// "list"}` or `{"action": "get", "hash":...}`).
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
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
    /// All torrents TorrServer currently knows about (`{"action": "list"}`).
    pub async fn list_torrents(&self) -> Result<Vec<TorrentInfo>> {
        let resp = self
            .torrents_action(serde_json::json!({ "action": "list" }))
            .await?;
        // TorrServer returns `null` (not `[]`) when there are no torrents;
        // treat that the same as an empty list rather than an error.
        let list: Option<Vec<TorrentInfo>> = resp.json().await.unwrap_or(None);
        Ok(list.unwrap_or_default())
    }

    /// A single torrent's status (`{"action": "get", "hash":...}`).
    pub async fn get_torrent(&self, hash: &str) -> Result<Option<TorrentInfo>> {
        let resp = self
            .torrents_action(serde_json::json!({ "action": "get", "hash": hash }))
            .await?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        Ok(resp.json().await.ok())
    }

    /// Stop an active torrent's download/seeding without forgetting it.
    pub async fn pause(&self, hash: &str) -> Result<()> {
        let body = serde_json::json!({ "action": "drop", "hash": hash });
        let resp = self.torrents_action(body).await?;
        ensure_ok(resp, "pause the torrent").await
    }

    /// Resume a paused (dropped) torrent.
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
            // "error parse link:..."), so surface it instead of a bare
            // status code.
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
        cmd.arg(&stream_url)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Start the player in its own process group so it survives doris
        // exiting (no SIGHUP propagation from the terminal).
        #[cfg(unix)]
        cmd.process_group(0);

        let child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to launch {}: {}", player_name, e))?;

        Ok(child)
    }
}
