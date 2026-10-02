//! The tracked torrent: starting a stream, pausing, removing, downloading a row.

use super::*;
use std::path::PathBuf;

impl App {
    /// Pause (drop) or resume (re-get) the torrent the panel is currently showing.
    pub(super) async fn toggle_pause_active_torrent(&mut self) {
        let Some(hash) = self.ui.active_torrent_hash.clone() else {
            self.ui.add_log("No active torrent to pause/resume.");
            return;
        };
        if self.ui.torrent_paused {
            match self.torrserver.resume(&hash).await {
                Ok(_) => {
                    self.ui.torrent_paused = false;
                    self.ui.add_log("Torrent resumed.");
                }
                Err(e) => self.ui.add_log(&format!("Resume failed: {}", e)),
            }
        } else {
            match self.torrserver.pause(&hash).await {
                Ok(_) => {
                    self.ui.torrent_paused = true;
                    self.ui.add_log("Torrent paused.");
                }
                Err(e) => self.ui.add_log(&format!("Pause failed: {}", e)),
            }
        }
    }

    pub(super) async fn remove_active_torrent(&mut self) {
        let Some(hash) = self.ui.active_torrent_hash.take() else {
            self.ui.add_log("No active torrent to remove.");
            return;
        };
        match self.torrserver.remove(&hash).await {
            Ok(_) => {
                self.ui.torrent_status = TorrentStatus::default();
                self.ui.torrent_paused = false;
                self.ui.progress_history.clear();
                self.ui.add_log("Torrent removed.");
            }
            Err(e) => self.ui.add_log(&format!("Remove failed: {}", e)),
        }
    }

    /// Fetch a result's `.torrent` bytes through the `Source` that actually owns it, instead of
    /// always going through rutracker's browser session -- which for a rutor row either failed
    /// ("No browser session") or fetched `rutor.org/download/...` cross-origin from a rutracker
    /// page.
    pub(super) async fn download_bytes_for(
        item: &crate::sources::models::TorrentItem,
        source: &dyn Source,
    ) -> Result<Vec<u8>> {
        source.download_torrent(&item.download_url).await
    }

    /// Download the selected result's .torrent file to disk (Options ->
    /// download's resolved directory), dispatching to whichever Source
    /// actually produced it -- `TorrentItem.source` matters here because
    /// the "all" Results tab can mix rows from more than one source at
    /// once, each needing a different download client.
    ///
    /// Returns where it went. The TUI ignores that and reads the log panel
    /// it just wrote, which is what a panel is for; a CLI command has to
    /// print the path, and reading a formatted log line back to recover a
    /// value the function already had is how the answer came out wrong
    /// once already -- `add_log` prefixes a timestamp, so the prefix a
    /// reader looks for is not at the front of the line.
    pub(super) async fn download_selected_to_disk(&mut self) -> Option<PathBuf> {
        let Some(item) = self.ui.results.get(self.ui.selected).cloned() else {
            self.ui.add_log("No result selected to download.");
            return None;
        };

        if !self.config.download_enabled {
            self.ui
                .add_log("Downloading is disabled in Options -> download -> Enable downloading.");
            return None;
        }

        self.ui.add_log(&format!("Downloading '{}'...", item.title));

        // A row with neither link nor file (1337x, B8 wave 3) reads its
        let mut item = item;
        if item.magnet.is_none() && item.download_url.is_empty() {
            let resolved = match self.get_source(source_id_for(&item)).await {
                Ok(source) => fill_missing_magnet(&mut item, source.as_ref()).await,
                Err(e) => Err(e),
            };
            if let Err(e) = resolved {
                self.ui
                    .add_log(&format!("Could not read the magnet link: {e}"));
                return None;
            }
            if item.magnet.is_none() {
                self.ui
                    .add_log("This row carries no magnet and no .torrent link.");
                return None;
            }
        }

        // A row with no `.torrent` to fetch (YTS and friends, B8 wave 1)
        if let Some((name, payload)) = magnet_only_download(&item) {
            let dir = self.resolve_download_dir();
            let path = std::path::Path::new(&dir).join(&name);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            return match std::fs::write(&path, payload.as_bytes()) {
                Ok(_) => {
                    self.ui
                        .add_log(&format!("Saved magnet link to {}", path.display()));
                    self.ui.last_download = Some(path.clone());
                    Some(path)
                }
                Err(e) => {
                    self.ui.add_log(&format!("Failed to save file: {e}"));
                    None
                }
            };
        }

        // `get_source` launches the browser only for sources whose
        let bytes_result: Result<Vec<u8>> = match self.get_source(source_id_for(&item)).await {
            Ok(source) => Self::download_bytes_for(&item, source.as_ref()).await,
            Err(e) => Err(e),
        };

        match bytes_result {
            Ok(bytes) => {
                let dir = self.resolve_download_dir();
                let path = std::path::Path::new(&dir)
                    .join(format!("{}.torrent", safe_filename(&item.title)));
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match std::fs::write(&path, &bytes) {
                    Ok(_) => {
                        self.ui.add_log(&format!("Saved to {}", path.display()));
                        self.ui.last_download = Some(path.clone());
                        Some(path)
                    }
                    Err(e) => {
                        self.ui.add_log(&format!("Failed to save file: {e}"));
                        None
                    }
                }
            }
            Err(e) => {
                self.ui.add_log(&format!("Download failed: {e}"));
                None
            }
        }
    }

    pub(super) async fn spawn_stream(&mut self) {
        if self.ui.selected >= self.ui.results.len() {
            self.ui.add_log("No result selected");
            return;
        }
        let mut item = self.ui.results[self.ui.selected].clone();
        self.ui.state = AppState::Streaming;

        // The source is here to read a magnet off the row's page, and only for a
        // row that has none. A row that already carries a magnet -- every
        // one of them from a JSON API source, and every one named by
        // `doris play --magnet` -- needs nothing from a tracker, and asking
        // for it means launching a browser to do nothing. So the source is
        // built only when it will be used, and its absence is not an error.
        let needs_source = item.magnet.is_none() && item.download_url.is_empty();
        let source = if needs_source {
            match self.source_for_row(source_id_for(&item)).await {
                Ok(s) => Some(s),
                Err(e) => {
                    self.ui.add_log(&e.to_string());
                    self.ui.state = AppState::Idle;
                    return;
                }
            }
        } else {
            None
        };

        let torrserver = self.torrserver.clone();
        let event_tx = self.event_handler.sender();
        let torrserver_enabled = self.config.enable_torrserver;

        tokio::spawn(async move {
            let log = |msg: &str| {
                let _ = event_tx.send(Event::StreamLog(msg.to_string()));
            };

            if !torrserver_enabled {
                // The user switched TorrServer off in Options: say so
                log("TorrServer is disabled in Options -> streaming -> Enable TorrServer.");
                let _ = event_tx.send(Event::StreamError(
                    "TorrServer is disabled in Options".into(),
                ));
                return;
            }

            if !torrserver.is_reachable().await {
                log(&format!(
                    "TorrServer is not reachable! Start TorrServer on {}",
                    crate::torrserver::api::DEFAULT_URL
                ));
                let _ = event_tx.send(Event::StreamError("TorrServer unreachable".into()));
                return;
            }

            // How the torrent reaches TorrServer: a row carrying a
            // magnet goes in by link, a row with only a `.torrent` gets it
            // uploaded. Neither needs the tracker once the link is in hand.
            if let Some(source) = source.as_ref() {
                if let Err(e) = fill_missing_magnet(&mut item, source.as_ref()).await {
                    log(&format!("Reading the magnet link failed: {e}"));
                }
            }
            if item.magnet.is_none() && item.download_url.is_empty() {
                let msg = "This row carries no magnet and no .torrent link.";
                log(msg);
                let _ = event_tx.send(Event::StreamError(msg.into()));
                return;
            }

            let mut linked: Option<String> = None;
            if let Some(magnet) = item.magnet.as_deref() {
                log(&format!("Adding by magnet link: {}", item.title));
                match torrserver.add_by_link(magnet, &item.title).await {
                    Ok(hash) => {
                        log(&format!("Added by link, hash: {}", hash));
                        linked = Some(hash);
                    }
                    Err(e) if item.download_url.is_empty() => {
                        // Nothing to fall back to: this row's only path
                        let msg = format!(
                            "Magnet add failed ({}), and this row has no .torrent \
                             to fall back to",
                            e
                        );
                        log(&msg);
                        let _ = event_tx.send(Event::StreamError(msg));
                        return;
                    }
                    Err(e) => {
                        log(&format!(
                            "Magnet add failed ({}); fetching .torrent instead",
                            e
                        ));
                    }
                }
            }

            let hash = match linked {
                Some(hash) => hash,
                None => {
                    // Reached only when the magnet add failed, so the
                    // `.torrent` has to be fetched -- and fetching it is
                    // the one thing that needs the tracker. A row named
                    // by `--magnet` never gets here: it had nothing to
                    // fail, so there was no source to build.
                    let Some(source) = source.as_ref() else {
                        let msg = "The magnet link was refused and no tracker is known for \
                                   this row, so the .torrent cannot be fetched.";
                        log(msg);
                        let _ = event_tx.send(Event::StreamError(msg.into()));
                        return;
                    };
                    log(&format!("Fetching .torrent file: {}", item.title));
                    let bytes = match Self::download_bytes_for(&item, source.as_ref()).await {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            log(&format!("Download error: {}", e));
                            let _ = event_tx.send(Event::StreamError(e.to_string()));
                            return;
                        }
                    };
                    log(&format!("Downloaded {} bytes", bytes.len()));
                    match torrserver.upload_torrent(&bytes, &item.title).await {
                        Ok(hash) => {
                            log(&format!("Uploaded, hash: {}", hash));
                            hash
                        }
                        Err(e) => {
                            log(&format!("Upload error: {}", e));
                            let _ = event_tx.send(Event::StreamError(e.to_string()));
                            return;
                        }
                    }
                }
            };

            let _ = event_tx.send(Event::TorrentActive(hash.clone()));
            match torrserver.play(&hash, &item.title, None).await {
                Ok(mut child) => {
                    let stream_url = format!("{}/stream/{}", torrserver.base_url(), hash);
                    let _ = event_tx.send(Event::StreamComplete(stream_url));

                    if let Some(stderr) = child.stderr.take() {
                        use tokio::io::{AsyncBufReadExt, BufReader};
                        let mut reader = BufReader::new(stderr).lines();
                        while let Ok(Some(line)) = reader.next_line().await {
                            let l = line.trim();
                            if l.is_empty() {
                                continue;
                            }
                            if crate::player_log::should_log(l) {
                                log(&format!("MPV: {}", l));
                            }
                        }
                    }
                }
                Err(e) => {
                    log(&format!("Player error: {}", e));
                    let _ = event_tx.send(Event::StreamError(e.to_string()));
                }
            }
        });
    }

    /// Show the selected result's full details in the log -- the 'v'
    /// action from the Results panel's bottom action row.
    pub(super) fn show_selected_info(&mut self) {
        let Some(item) = self.ui.results.get(self.ui.selected) else {
            self.ui.add_log("No result selected.");
            return;
        };
        let source = if item.source.is_empty() {
            "rutracker"
        } else {
            item.source.as_str()
        };
        self.ui.add_log(&format!(
            "INFO: {}  |  size={}  seeds={}  date={}  source={}  url={}",
            item.title, item.size, item.seeds, item.date, source, item.page_url,
        ));
    }

    /// Open the detail modal for the selected row Shift+Enter and ask its source for the file
    /// list.
    pub(super) async fn open_detail_modal(&mut self) {
        let Some(item) = self.ui.results.get(self.ui.selected).cloned() else {
            self.ui.add_log("No result selected.");
            return;
        };
        self.ui.modal = Modal::TorrentDetail(Box::new(TorrentDetailState::new(item.clone())));

        let source = match self.get_source(source_id_for(&item)).await {
            Ok(source) => source,
            Err(e) => {
                self.ui.add_log(&e.to_string());
                return;
            }
        };
        let page_url = item.page_url.clone();
        let tx = self.event_handler.sender();
        tokio::spawn(async move {
            let outcome = source.details(&page_url).await;
            let (files, error) = match outcome {
                Ok(files) => (files, None),
                Err(e) => (Vec::new(), Some(e.to_string())),
            };
            let _ = tx.send(Event::DetailLoaded {
                page_url,
                files,
                error,
            });
        });
    }

    /// See the free function of the same name for the resolution logic;
    /// this just supplies `&self.config`.
    pub(super) fn resolve_download_dir(&self) -> String {
        resolve_download_dir(&self.config)
    }
}
