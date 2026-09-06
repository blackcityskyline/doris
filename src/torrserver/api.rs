use anyhow::Result;
use reqwest::Client;

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

    pub async fn play(&self, hash: &str, title: &str, player: Option<&str>) -> Result<String> {
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
        let _ = tokio::process::Command::new(player_name)
            .arg(&stream_url)
            .spawn();

        Ok(stream_url)
    }
}
