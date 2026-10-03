//! The tracked torrent: starting a stream, pausing, removing, downloading a row.

use super::*;
use std::path::PathBuf;

impl App {
    /// Pause (drop) or resume (re-get) the torrent the panel is currently showing.
    /// Pause or resume the download under the Torrents panel's cursor.
    ///
    /// The panel is a list of what the daemon is fetching, so this acts on
    /// that list and not on `active_torrent_hash` -- which is the streaming
    /// server's one torrent, a different machine state on a different service.
    /// Before the panel was a list these were the same thing, and now they
    /// are not.
    pub(super) async fn toggle_pause_download(&mut self) {
        let Some(row) = self.ui.downloads.get(self.ui.download_cursor).cloned() else {
            self.ui.add_log("No download selected.");
            return;
        };
        let paused = row.status == 0;
        let outcome = if paused {
            self.transmission.resume(row.id).await
        } else {
            self.transmission.pause(row.id).await
        };
        match outcome {
            Ok(()) => self.ui.add_log(&format!(
                "{} {}",
                if paused { "resumed" } else { "paused" },
                row.name
            )),
            Err(e) => self
                .ui
                .add_log(&format!("Could not pause {}: {e}", row.name)),
        }
    }

    /// Remove the download under the cursor, asking the daemon whether to
    /// delete what it has fetched.
    pub(super) async fn remove_download(&mut self) {
        let Some(row) = self.ui.downloads.get(self.ui.download_cursor).cloned() else {
            self.ui.add_log("No download selected.");
            return;
        };
        match self.transmission.remove(row.id, true).await {
            Ok(()) => {
                self.ui
                    .add_log(&format!("Removed {} and its data", row.name));
                // Take the row off the list here rather than waiting for the
                // next poll: the key should feel like it did something.
                self.ui.downloads.retain(|r| r.id != row.id);
                self.ui.download_cursor = self
                    .ui
                    .download_cursor
                    .min(self.ui.downloads.len().saturating_sub(1));
            }
            Err(e) => self
                .ui
                .add_log(&format!("Could not remove {}: {e}", row.name)),
        }
    }

    /// The Torrents detail view's keys, on the row under its cursor.
    ///
    /// The zone version of `p` and `d` already existed; the rest are what a
    /// full-frame list of downloads is missing to be a client rather than a
    /// readout: check the data, open what was fetched, cap the rate.
    pub(super) async fn torrent_detail_key(&mut self, code: KeyCode) -> Result<()> {
        /// The keys that act on a row, and so need a row.
        const ROW_KEYS: &[char] = &['p', 'd', 'v', 'o', 'f', '+', '=', '-', '0'];

        let Some(row) = self.ui.downloads.get(self.ui.download_cursor).cloned() else {
            if matches!(code, KeyCode::Char(c) if ROW_KEYS.contains(&c)) {
                self.ui.add_log("No download to act on.");
            }
            return Ok(());
        };
        match code {
            KeyCode::Char('p') => self.toggle_pause_download().await,
            KeyCode::Char('d') => {
                self.ui.confirm_remove();
            }
            KeyCode::Char('v') => match self.transmission.verify(row.id).await {
                Ok(()) => self.ui.add_log(&format!("Verifying {}", row.name)),
                Err(e) => self
                    .ui
                    .add_log(&format!("Could not verify {}: {e}", row.name)),
            },
            KeyCode::Char('o') => self.open_download_dir(&row),
            KeyCode::Char('f') => {
                // Opened before the daemon answers, so the key has an
                // immediate effect: a modal that appears a second later is
                // a key that looks like it did nothing.
                self.ui.open_files_modal(row.id, row.name.clone());
                self.spawn_files_fetch(row.id);
            }
            KeyCode::Char('+') | KeyCode::Char('=') => self.step_download_limit(&row, 1).await,
            KeyCode::Char('-') => self.step_download_limit(&row, -1).await,
            KeyCode::Char('0') => self.set_download_limit(&row, None).await,
            _ => {}
        }
        Ok(())
    }

    /// Open what a download wrote, in whatever file manager this machine
    /// has.
    ///
    /// The daemon is asked where it put the files rather than the config
    /// being consulted, because a torrent added from the CLI or by another
    /// client can be anywhere.
    fn open_download_dir(&mut self, row: &crate::ui::view::DownloadRow) {
        let dir = row.dir.trim();
        if dir.is_empty() {
            self.ui.add_log(&format!("No directory for {}", row.name));
            return;
        }
        if !std::path::Path::new(dir).exists() {
            self.ui.add_log(&format!("{dir} is not there any more"));
            return;
        }
        // The setting picks the manager, and `pick` also says which one it
        // wanted but could not have: a name that is not installed says so
        // and falls back, rather than quietly running something else.
        let (manager, missing) = crate::app::files::pick(&self.config.file_manager);
        let Some(manager) = manager else {
            self.ui.add_log(&format!("Nothing here can open {dir}"));
            return;
        };
        if let Some(missing) = missing {
            self.ui.add_log(&format!(
                "{missing} is not installed; opening with {}",
                manager.program
            ));
        }
        self.ui
            .add_log(&format!("Opening {} in {}", dir, manager.program));
        let _ = self.event_handler.sender().send(Event::OpenPath {
            manager,
            path: dir.to_string(),
        });
    }

    async fn set_download_limit(&mut self, row: &crate::ui::view::DownloadRow, limit: Option<i64>) {
        let said = match self.transmission.set_download_limit(row.id, limit).await {
            Ok(()) => match limit {
                Some(bytes) => format!(
                    "{} limited to {}",
                    row.name,
                    crate::transmission::human_speed(bytes)
                ),
                None => format!("{} unlimited", row.name),
            },
            Err(e) => format!("Could not set the limit on {}: {e}", row.name),
        };
        self.ui.add_log(&said);
    }

    /// Move the download rate one notch along the steps a person picks from.
    ///
    /// Unlimited is the top rung rather than a separate state above it: a
    /// ramp with a step off the end of it is a ramp `+` walks up and never
    /// comes back down, which is what a limit that cannot be lifted looks
    /// like. So `0` is a shortcut for the same place, not a third thing.
    async fn step_download_limit(&mut self, row: &crate::ui::view::DownloadRow, dir: i64) {
        let steps = crate::app::files::SPEED_STEPS;
        let top = steps.len() as i64;
        let current = match row.limit_bytes {
            Some(bytes) => steps
                .iter()
                .position(|s| *s >= bytes)
                .map(|i| i as i64)
                .unwrap_or(0),
            None => top,
        };
        let next = (current + dir).clamp(0, top);
        let limit = (next < top).then(|| steps[next as usize]);
        self.set_download_limit(row, limit).await;
    }

    /// Ask the daemon for one download's files, off the key path.
    ///
    /// A spawn rather than an await because the key that opened the modal
    /// must not wait on a network round trip: the modal appears at once
    /// with a placeholder and the answer arrives as an event.
    pub(super) fn spawn_files_fetch(&mut self, id: i64) {
        let tx = self.event_handler.sender();
        let client = self.transmission.clone();
        tokio::spawn(async move {
            let event = match client.files(id).await {
                Ok(files) => Event::DownloadFiles {
                    id,
                    files,
                    error: None,
                },
                Err(e) => Event::DownloadFiles {
                    id,
                    files: Vec::new(),
                    error: Some(e.to_string()),
                },
            };
            let _ = tx.send(event);
        });
    }

    /// Send the file switches the modal made, and forget them.
    ///
    /// Drained here because this is the layer that can reach Transmission;
    /// the modal only records what the user asked for. Each file is its own
    /// call because Transmission's `torrent-set` takes the wanted list as
    /// indices, and a torrent of a hundred files is a hundred entries to
    /// turn on -- batched, it is one call with the list of the ones that
    /// changed, which is what the daemon reads.
    pub(super) async fn send_pending_file_wants(&mut self) {
        let pending = std::mem::take(&mut self.ui.pending_files);
        if pending.is_empty() {
            return;
        }
        let Modal::Files(state) = &self.ui.modal else {
            return;
        };
        let id = state.id;
        let name = state.name.clone();
        let mut failed = Vec::new();
        for (index, wanted) in pending {
            if let Err(e) = self.transmission.set_file_wanted(id, index, wanted).await {
                failed.push(format!("{index}: {e}"));
            }
        }
        if failed.is_empty() {
            self.ui.add_log(&format!("Files of {name} updated"));
        } else {
            self.ui.add_log(&format!(
                "Could not set files of {name}: {}",
                failed.join(", ")
            ));
        }
    }

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
        // Enter streams; it does not fetch. TorrServer keeps its own list
        // and knows nothing of what Transmission holds, so a torrent that is
        // being downloaded still has to be handed to TorrServer before it
        // can be streamed -- which is exactly what the path below does.
        // Stopping to check the download list here would be wrong twice
        // over: it would skip the hand-over, and it would answer a
        // question this key does not ask.
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
        // For the message when it is not answering: naming the default port
        // to someone who moved theirs is a wrong answer about their machine.
        let torrserver_url = self.config.torrserver_url.clone();
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
                // The URL the user configured, not the default: telling
                // someone to start TorrServer on 8090 when theirs is on
                // 8091 is a message about their machine that is wrong.
                log(&format!(
                    "Nothing is listening on {}. Start it, or point \
                     `torrserver_url` in config.toml at where it is.",
                    torrserver_url
                ));
                let _ = event_tx.send(Event::StreamError(format!(
                    "TorrServer unreachable at {torrserver_url}"
                )));
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
