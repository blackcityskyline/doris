//! Transmission's RPC, as a client.
//!
//! The other half of the pair with [`crate::torrserver::api`]: TorrServer
//! streams, Transmission downloads. Neither can do the other's job, which
//! is measured rather than assumed -- TorrServer's own README calls it a
//! viewer that "allows users to view torrents online without the need for
//! preliminary file downloading", it fetches only what a reader asks for,
//! caches it under `<hash>/<chunk>`, and drops the torrent thirty seconds
//! after the last reader goes away. Setting `UseDisk` and a save path
//! changes where the cache lives, not whether it is a cache.
//!
//! Transmission answers `4.1.x`, rpc version 19. The awkward part of its
//! protocol is the session handshake: the first request is refused with
//! `409` and a header naming a token, and every request after it must
//! carry that token. `call` does it, so nothing above this file has to
//! know it exists.
//!
//! A note on what this prints. Cookies and passwords are a session, and
//! `auth` reads them out of the environment -- a CLI's arguments are
//! visible in `ps` to everything on the machine, so the password is never
//! accepted as one.

pub mod adopt;
pub mod downloads_cmd;
pub mod poller;

use anyhow::{bail, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const DEFAULT_URL: &str = "http://127.0.0.1:9091";

/// One torrent, in the shape `torrent-get` reports it.
///
/// Field names are Transmission's own, renamed once here so that nothing
/// above has to know which ones are renamed. `progress` is a fraction,
/// `ratio` is uploaded/divided-by-downloaded, and `eta` is seconds.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Download {
    pub id: i64,
    /// The info hash, lower-case hex. This is the join key: a search row
    /// and a download meet on it and nothing else.
    #[serde(rename = "hashString", default)]
    pub hash: String,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "percentDone", default)]
    pub fraction: f64,
    #[serde(rename = "downloadSpeed", default)]
    pub download_speed: i64,
    #[serde(rename = "uploadSpeed", default)]
    pub upload_speed: i64,
    #[serde(rename = "peersConnected", default)]
    pub peers: i64,
    #[serde(rename = "peersSendingToUs", default)]
    pub seeds: i64,
    #[serde(rename = "sizeWhenDone", default)]
    pub total_size: i64,
    #[serde(rename = "leftUntilDone", default)]
    pub left: i64,
    #[serde(rename = "addedDate", default)]
    pub added: i64,
    #[serde(rename = "downloadDir", default)]
    pub dir: String,
    /// Transmission's status code: 0 stopped, 1 check pending, 2 checking,
    /// 3 download pending, 4 downloading, 5 seed pending, 6 seeding.
    #[serde(default)]
    pub status: i64,
    #[serde(rename = "isFinished", default)]
    pub finished: bool,
    #[serde(rename = "errorString", default)]
    pub error: String,
    #[serde(rename = "uploadedEver", default)]
    pub uploaded: i64,
    #[serde(rename = "downloadEver", default)]
    pub downloaded: i64,
    #[serde(rename = "eta", default)]
    pub eta: i64,
    #[serde(rename = "trackerStats", default)]
    pub trackers: Vec<TrackerStat>,
    /// Bytes per second this torrent may use, and whether it may use them.
    /// Zero with the flag off is "no limit of its own", which is not the
    /// same as a limit of zero: one is a speed, the other is a stop.
    ///
    /// Read from Transmission 4's `downloadLimit`, which is in kilobytes
    /// per second, and kept in bytes because that is what every other
    /// number in this file is in and a panel that has to divide by 1024 to
    /// compare two of its own fields is a panel nobody edits. The older
    /// `speedLimitDown` pair is *accepted* by Transmission 4 and silently
    /// ignored -- no error, no effect -- so it is not used.
    #[serde(rename = "downloadLimit", default)]
    pub download_limit_kbps: i64,
    #[serde(rename = "downloadLimited", default)]
    pub limited: bool,
}

/// What one tracker reports about a torrent.
///
/// The three fields keep Transmission's names because the other wire types
/// keep theirs too, and a mix of renamed and not would make the `rename`
/// attributes a lookup table to check rather than a rule. This one is
/// private to the module, so nothing outside ever writes the spelling.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TrackerStat {
    #[serde(default)]
    pub host: String,
    #[serde(rename = "seederCount", default)]
    pub seeders: i64,
    #[serde(rename = "leecherCount", default)]
    pub leechers: i64,
    #[serde(rename = "lastAnnounceSucceeded", default)]
    pub announced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileEntry {
    /// `name` is the path within the torrent, not a basename: a torrent of
    /// a season has directories in here.
    #[serde(default)]
    pub name: String,
    #[serde(rename = "length", default)]
    pub size: i64,
    #[serde(rename = "bytesCompleted", default)]
    pub done: f64,
    /// Whether this file is being fetched. Transmission reports it per
    /// file, which is the only way to ask for part of a torrent -- the unit
    /// of choice is the file, not the torrent.
    #[serde(default = "default_true")]
    pub wanted: bool,
}

/// Hand-written, because `#[derive(Default)]` and `#[serde(default)]` would
/// disagree: serde's default for `wanted` is true (a file is fetched unless
/// someone says otherwise) while the derive's is false. A `FileEntry` built
/// in a test or by hand would then be a file nobody wants, which is the
/// opposite of what an absent answer means.
impl Default for FileEntry {
    fn default() -> Self {
        Self {
            name: String::new(),
            size: 0,
            done: 0.0,
            wanted: true,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Copy `fileStats`' `wanted` flags onto the file list.
///
/// Separate from [`Transmission::files`] so the rule can be checked without
/// a daemon: the two arrays are parallel and both indexed by position, and
/// a `fileStats` shorter than `files` leaves the rest wanted rather than
/// marking them unwanted -- a file the daemon did not mention is a file it
/// has no opinion about, not one the user turned off.
pub fn merge_wanted(files: &mut [FileEntry], file_stats: Option<&serde_json::Value>) {
    let Some(stats) = file_stats.and_then(|s| s.as_array()) else {
        return;
    };
    for (index, file) in files.iter_mut().enumerate() {
        if let Some(wanted) = stats
            .get(index)
            .and_then(|s| s.get("wanted"))
            .and_then(|w| w.as_bool())
        {
            file.wanted = wanted;
        }
    }
}

impl Download {
    /// 0.0-1.0, as a percentage for a person.
    pub fn percent(&self) -> f64 {
        (self.fraction * 100.0).clamp(0.0, 100.0)
    }

    /// Uploaded over downloaded. `None` before anything has been
    /// downloaded, where the arithmetic has no answer and `0.00` would
    /// look like a measurement.
    pub fn ratio(&self) -> Option<f64> {
        if self.downloaded <= 0 {
            return None;
        }
        Some(self.uploaded as f64 / self.downloaded as f64)
    }

    /// A human ETA, `None` when Transmission is not predicting one.
    pub fn eta_text(&self) -> Option<String> {
        match self.eta {
            e if e < 0 => None,
            0 if self.left > 0 => Some(format!("{} left", human_bytes(self.left as u64))),
            0 => Some("done".to_string()),
            secs => Some(format!("{} left", eta_secs(secs))),
        }
    }

    /// The word for what this torrent is doing, from its status code.
    /// The limit in bytes per second, which is what the panel and the
    /// `+`/`-` steps speak.
    pub fn limit_bytes(&self) -> Option<i64> {
        self.limited.then(|| self.download_limit_kbps.max(0) * 1024)
    }

    pub fn state(&self) -> &'static str {
        state_of(self.status)
    }

    /// Total uploaded across every tracker.
    pub fn tracker_seeds(&self) -> i64 {
        self.trackers.iter().map(|t| t.seeders).sum()
    }
}

/// A speed the way a column shows one: no unit at zero, so a column of
/// zeros is not a column of `0.0 B/s`.
pub fn human_speed(bytes: i64) -> String {
    let b = bytes.max(0) as u64;
    // A column of zeros should not be a column of `0.0 B/s`.
    if b == 0 {
        return "0".to_string();
    }
    format!("{}/s", human_bytes(b))
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// The word for a Transmission status code.
///
/// Transmission's codes are the contract; the words are ours, and the TUI
/// and the CLI have to say the same thing about the same number -- so this
/// is a free function rather than a method, because the panel's row type
/// lives in the ui module and must not depend on this one to ask it a
/// question.
pub fn state_of(status: i64) -> &'static str {
    match status {
        0 => "paused",
        1 => "queued to verify",
        2 => "verifying",
        3 => "queued to download",
        4 => "downloading",
        5 => "queued to seed",
        _ => "seeding",
    }
}

/// A duration the way a person reads one: `45s`, `5m`, `1h 1m`, `2d`.
pub fn eta_secs(secs: i64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    if secs < 3600 {
        return format!("{}m", secs / 60);
    }
    if secs < 86400 {
        let (hours, minutes) = (secs / 3600, (secs % 3600) / 60);
        // "1h 0m" is noise: an hour with nothing after it is just an hour.
        return if minutes == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h {minutes}m")
        };
    }
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    if hours == 0 {
        format!("{days}d")
    } else {
        format!("{days}d {hours}h")
    }
}

/// One `torrent-add` result.
#[derive(Debug, Clone, Deserialize)]
struct AddResponse {
    #[serde(rename = "torrent-added")]
    added: Option<Download>,
    #[serde(rename = "torrent-duplicate")]
    duplicate: Option<Download>,
}

/// What `add` did, which is not the same as "added it".
#[derive(Debug, Clone, PartialEq)]
pub enum Added {
    /// It was not there and is now.
    Fresh(i64),
    /// It was already there. Re-adding is a no-op, not an error: a
    /// search result the user already has is not an error, it is the
    /// answer.
    AlreadyThere(i64),
    /// Transmission refused, and said why.
    Refused(String),
}

#[derive(Clone)]
pub struct Transmission {
    client: Client,
    url: String,
    auth: Option<(String, String)>,
    /// The token from the last `409`, kept for the life of the client.
    ///
    /// Cached rather than refetched because Transmission rotates it, and a
    /// second call in the same second should not have to pay for a
    /// handshake it already did.
    session: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl Transmission {
    pub fn new(url: &str) -> Self {
        Self::with_auth(url, None)
    }

    /// `auth` is `(user, password)`. Transmission only wants it when
    /// `--auth` was passed to the daemon, so this is `None` by default and
    /// the common case costs nothing.
    pub fn with_auth(url: &str, auth: Option<(String, String)>) -> Self {
        Self {
            client: Client::new(),
            url: url.trim_end_matches('/').to_string(),
            auth,
            session: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.url
    }

    pub async fn is_reachable(&self) -> bool {
        self.client
            .get(&self.url)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    async fn rpc(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        let mut attempt = 0;
        loop {
            let mut req = self
                .client
                .post(format!("{}/transmission/rpc", self.url))
                .json(&body)
                .timeout(Duration::from_secs(10));
            if let Some((user, password)) = &self.auth {
                req = req.basic_auth(user, Some(password));
            }
            let token = self.session.lock().ok().and_then(|g| g.clone());
            if let Some(token) = token {
                req = req.header("X-Transmission-Session-Id", token);
            }

            let resp = req.send().await?;
            if resp.status() == reqwest::StatusCode::CONFLICT {
                // The handshake: Transmission answers the first request
                // with 409 and the token to use from now on.
                if let Ok(mut guard) = self.session.lock() {
                    *guard = resp
                        .headers()
                        .get("X-Transmission-Session-Id")
                        .and_then(|v| v.to_str().ok().map(str::to_string));
                }
                attempt += 1;
                if attempt > 2 {
                    bail!("Transmission kept asking for a new session token");
                }
                continue;
            }
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                bail!(
                    "Transmission refused {method}: {status} {reason}",
                    method = body.get("method").and_then(|m| m.as_str()).unwrap_or("?"),
                    reason = text.trim()
                );
            }
            let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
                anyhow::anyhow!("Transmission sent something that is not JSON: {e}")
            })?;
            if value.get("result").and_then(|r| r.as_str()) == Some("error") {
                bail!(
                    "Transmission refused {}: {}",
                    body.get("method").and_then(|m| m.as_str()).unwrap_or("?"),
                    value
                        .get("result")
                        .and_then(|r| r.get("arguments"))
                        .and_then(|a| a.get("result"))
                        .and_then(|r| r.as_str())
                        .unwrap_or("no reason given")
                );
            }
            return Ok(value);
        }
    }

    async fn call(&self, method: &str, arguments: serde_json::Value) -> Result<serde_json::Value> {
        self.rpc(serde_json::json!({ "method": method, "arguments": arguments }))
            .await
    }

    /// The daemon's own version, which is what a health check reports and
    /// what a failure message needs: "Transmission refused" and
    /// "Transmission 2.94 refused" are different bugs.
    pub async fn version(&self) -> Result<String> {
        let out = self
            .call("session-get", serde_json::json!({ "fields": ["version"] }))
            .await?;
        Ok(out
            .get("arguments")
            .and_then(|a| a.get("version"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string())
    }

    /// How the daemon is doing, in the three numbers a status panel shows.
    pub async fn free_space(&self) -> Result<i64> {
        let out = self
            .call(
                "session-get",
                serde_json::json!({ "fields": ["download-dir-free-space"] }),
            )
            .await?;
        Ok(out
            .get("arguments")
            .and_then(|a| a.get("download-dir-free-space"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0))
    }

    /// Every torrent the daemon holds, newest field set and all.
    ///
    /// `fields` is spelled out rather than left to Transmission's
    /// defaults: the defaults are a subset, and a status panel that shows
    /// a column Transmission happened to include by default is a panel
    /// that changes when Transmission does.
    pub async fn list(&self) -> Result<Vec<Download>> {
        let fields: Vec<&str> = vec![
            "id",
            "hashString",
            "name",
            "percentDone",
            "downloadSpeed",
            "uploadSpeed",
            "peersConnected",
            "peersSendingToUs",
            "sizeWhenDone",
            "leftUntilDone",
            "addedDate",
            "downloadDir",
            "status",
            "isFinished",
            "errorString",
            "uploadedEver",
            "downloadEver",
            "eta",
            "trackerStats",
            "downloadLimit",
            "downloadLimited",
            // Not `files`: a list of every file of every torrent is a lot
            // of JSON for a panel that shows one row's worth, and
            // `files()` asks for that torrent alone.
        ];
        let out = self
            .call("torrent-get", serde_json::json!({ "fields": fields }))
            .await?;
        let mut list: Vec<Download> = out
            .get("arguments")
            .and_then(|a| a.get("torrents"))
            .and_then(|t| serde_json::from_value(t.clone()).ok())
            .unwrap_or_default();
        // Transmission returns them in no order worth having. Newest
        // first, because "what did I just start" is the question a list is
        // usually asked.
        list.sort_by_key(|t| std::cmp::Reverse(t.added));
        Ok(list)
    }

    pub async fn get(&self, id: i64) -> Result<Option<Download>> {
        Ok(self.list().await?.into_iter().find(|t| t.id == id))
    }

    /// The one download with this info hash, if the daemon has it.
    ///
    /// This is the join: a search row carries an info hash and so does a
    /// download, so "is this result already being fetched?" is answered
    /// without an id either side ever having to be remembered.
    pub async fn find_by_hash(&self, hash: &str) -> Result<Option<Download>> {
        let wanted = hash.trim().to_ascii_lowercase();
        Ok(self
            .list()
            .await?
            .into_iter()
            .find(|t| t.hash.eq_ignore_ascii_case(&wanted)))
    }

    /// Add a magnet or a `.torrent` URL, optionally into a directory.
    ///
    /// Says what it did rather than only that it worked: Transmission
    /// answers `torrent-duplicate` for something already held, and a
    /// caller that treated that as an error would report a failure for
    /// the one case where nothing needed doing.
    pub async fn add(&self, link: &str, dir: Option<&str>) -> Result<Added> {
        let mut args = serde_json::json!({ "filename": link });
        if let Some(dir) = dir {
            args["download-dir"] = serde_json::json!(dir);
        }
        let out = self.call("torrent-add", args).await?;
        let parsed: AddResponse = serde_json::from_value(
            out.get("arguments").cloned().unwrap_or_default(),
        )
        .unwrap_or(AddResponse {
            added: None,
            duplicate: None,
        });
        if let Some(t) = parsed.added {
            return Ok(Added::Fresh(t.id));
        }
        if let Some(t) = parsed.duplicate {
            return Ok(Added::AlreadyThere(t.id));
        }
        Ok(Added::Refused(
            "Transmission took the request and named no torrent".into(),
        ))
    }

    /// Stop a download, keeping its place.
    ///
    /// `torrent-stop`, not `torrent-set` with `action: "stop"`. Both are
    /// written down and the second one answers `result: success` on a
    /// Transmission 4 while leaving the status at `4` -- downloading --
    /// and going on downloading. That is the worst shape a bug can take: a
    /// success message for a call that did nothing, which is how the panel
    /// ended up saying "paused" over a torrent that was not paused. The
    /// legacy method is the one that works, so it is the one used, and the
    /// live test asserts the status actually moved -- a green call is not
    /// evidence, the state is.
    pub async fn pause(&self, id: i64) -> Result<()> {
        self.call("torrent-stop", serde_json::json!({ "ids": [id] }))
            .await
            .map(|_| ())
    }

    pub async fn resume(&self, id: i64) -> Result<()> {
        self.call("torrent-start", serde_json::json!({ "ids": [id] }))
            .await
            .map(|_| ())
    }

    /// Move a download, carrying its data with it.
    pub async fn move_to(&self, id: i64, dir: &str) -> Result<()> {
        self.call(
            "torrent-set-location",
            serde_json::json!({ "ids": [id], "location": dir, "move": true }),
        )
        .await
        .map(|_| ())
    }

    /// Remove a download. `with_data` is the difference between taking it
    /// off the list and deleting what it downloaded, so it is asked for
    /// rather than defaulted: both are one flag apart and irreversible in
    /// different ways.
    pub async fn remove(&self, id: i64, with_data: bool) -> Result<()> {
        self.call(
            "torrent-remove",
            serde_json::json!({ "ids": [id], "delete-local-data": with_data }),
        )
        .await
        .map(|_| ())
    }

    /// Check the local data against the torrent's hashes.
    pub async fn verify(&self, id: i64) -> Result<()> {
        self.call("torrent-verify", serde_json::json!({ "ids": [id] }))
            .await
            .map(|_| ())
    }

    /// Cap one torrent's download rate. `None` is "no limit of its own",
    /// which is the flag off rather than a limit of zero -- zero would be a
    /// stop, and the two are one field apart.
    pub async fn set_download_limit(&self, id: i64, bytes_per_second: Option<i64>) -> Result<()> {
        self.call(
            "torrent-set",
            serde_json::json!({
                "ids": [id],
                // Kilobytes per second, because that is what Transmission 4
                // calls this field. Sending bytes here is accepted and
                // ignored, which is worse than an error: the key looks like
                // it worked.
                "downloadLimit": bytes_per_second.unwrap_or(0).max(0) / 1024,
                "downloadLimited": bytes_per_second.is_some(),
            }),
        )
        .await
        .map(|_| ())
    }

    /// Fetch, or stop fetching, one file of a torrent.
    pub async fn set_file_wanted(&self, id: i64, index: usize, wanted: bool) -> Result<()> {
        let key = if wanted {
            "files-wanted"
        } else {
            "files-unwanted"
        };
        self.call(
            "torrent-set",
            serde_json::json!({ "ids": [id], key: [index] }),
        )
        .await
        .map(|_| ())
    }

    /// The file list, asked for directly rather than read off `list`.
    ///
    /// `list` carries files for every torrent, which is a lot of JSON for
    /// a panel that shows one row's worth.
    ///
    /// Asked as `files` *and* `fileStats`, because Transmission 4 will not
    /// say whether a file is being fetched in `files` -- that array carries
    /// the name, the length and two piece numbers, and nothing about the
    /// choice. `fileStats` is where `wanted` lives, one entry per file, in
    /// the same order. Asking for only `files` gives a list where every
    /// entry looks wanted, which is a list that cannot show what the user
    /// just turned off.
    pub async fn files(&self, id: i64) -> Result<Vec<FileEntry>> {
        let out = self
            .call(
                "torrent-get",
                serde_json::json!({
                    "ids": [id],
                    "fields": ["files", "fileStats"],
                }),
            )
            .await?;
        let torrent = out
            .get("arguments")
            .and_then(|a| a.get("torrents"))
            .and_then(|t| t.get(0))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let mut files: Vec<FileEntry> = torrent
            .get("files")
            .and_then(|f| serde_json::from_value(f.clone()).ok())
            .unwrap_or_default();
        merge_wanted(&mut files, torrent.get("fileStats"));
        Ok(files)
    }

    /// Wait until a download is finished, `stop` says otherwise, or the
    /// deadline runs out.
    pub async fn watch(
        &self,
        id: i64,
        deadline: Duration,
        mut stop: impl FnMut(&Download) -> bool,
    ) -> Result<Option<Download>> {
        let give_up = tokio::time::Instant::now() + deadline;
        let mut last: Option<Download> = None;
        loop {
            let Some(current) = self.get(id).await? else {
                return Ok(last);
            };
            let done = current.finished || current.fraction >= 1.0;
            last = Some(current);
            if done || stop(last.as_ref().expect("stored just above")) {
                return Ok(last);
            }
            let Some(remaining) = give_up.checked_duration_since(tokio::time::Instant::now())
            else {
                return Ok(last);
            };
            tokio::time::sleep(remaining.min(Duration::from_secs(2))).await;
        }
    }
}
