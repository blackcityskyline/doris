//! The TorrServer modal's actions: play, pause/resume, remove, save, add.
//!
//! The modal only moves its cursor and returns what the user asked for;
//! these are the calls that reach the streaming server, which is why they
//! live with the app rather than with the view.

use super::*;
use crate::ui::modals::torrserver::{TorrServerAction, TorrServerDetailState};

impl App {
    pub(super) async fn apply_torrserver_action(&mut self, action: TorrServerAction) {
        match action {
            TorrServerAction::Play { hash, name } => self.play_torrserver(&hash, &name),
            TorrServerAction::SetPaused { hash, name, paused } => {
                self.set_torrserver_paused(&hash, &name, paused).await
            }
            TorrServerAction::Remove { hash, name } => self.remove_torrserver(&hash, &name).await,
            TorrServerAction::Save { torrent, file } => self.save_torrserver_file(&torrent, &file),
            TorrServerAction::SavePlaylist { hash, name } => {
                self.save_torrserver_playlist(&hash, &name)
            }
            TorrServerAction::Details { torrent } => self.open_torrserver_details(torrent),
        }
    }

    /// Start the player on a row the modal picked. The torrent is already
    /// on the server, so this is the one path that needs no source and no
    /// upload: it is what `Enter` on the Results table ends at, minus the
    /// five minutes of getting there.
    fn play_torrserver(&mut self, hash: &str, name: &str) {
        if self.ui.active_torrent_hash.as_deref() != Some(hash) {
            self.ui.progress_history.clear();
        }
        self.ui.active_torrent_hash = Some(hash.to_string());
        self.ui.add_log(&format!("Playing {name}"));
        crate::app::stream::spawn_player(
            self.torrserver.clone(),
            hash.to_string(),
            name.to_string(),
            self.event_handler.sender(),
        );
    }

    async fn set_torrserver_paused(&mut self, hash: &str, name: &str, paused: bool) {
        let outcome = if paused {
            self.torrserver.resume(hash).await
        } else {
            self.torrserver.pause(hash).await
        };
        match outcome {
            Ok(()) => {
                // Move the row at once; the poller will confirm it within
                // one tick, and a key that looks stuck is a key that
                // worked and said nothing.
                if let Some(row) = self
                    .ui
                    .torrserver_torrents
                    .iter_mut()
                    .find(|t| t.hash == hash)
                {
                    row.status_string = if paused {
                        "Torrent working".to_string()
                    } else {
                        "Torrent closed".to_string()
                    };
                }
                self.ui.add_log(&format!(
                    "{} {name}",
                    if paused { "Resumed" } else { "Paused" }
                ));
            }
            Err(e) => self.ui.add_log(&format!(
                "Could not {} {name}: {e}",
                if paused { "resume" } else { "pause" }
            )),
        }
    }

    async fn remove_torrserver(&mut self, hash: &str, name: &str) {
        match self.torrserver.remove(hash).await {
            Ok(()) => {
                self.ui.torrserver_torrents.retain(|t| t.hash != hash);
                self.ui.torrserver_state.cursor = self
                    .ui
                    .torrserver_state
                    .cursor
                    .min(self.ui.torrserver_torrents.len().saturating_sub(1));
                self.ui.torrserver_state.file_cursor = 0;
                if self.ui.active_torrent_hash.as_deref() == Some(hash) {
                    self.ui.active_torrent_hash = None;
                    self.ui.torrent_status = TorrentStatus::default();
                    self.ui.progress_history.clear();
                }
                self.ui.add_log(&format!("Removed {name} from TorrServer"));
            }
            Err(e) => self
                .ui
                .add_log(&format!("Could not remove {name} from TorrServer: {e}")),
        }
    }

    /// Write one file of a torrent under the download directory.
    ///
    /// The largest file is what `s` picks when the file list is closed --
    /// in a release that is the video. With `v` open it saves the row under
    /// the cursor instead, which is the only way to pick an episode out of
    /// a season pack.
    fn save_torrserver_file(
        &mut self,
        torrent: &crate::torrserver::api::TorrentInfo,
        file: &crate::torrserver::api::TorrentFile,
    ) {
        let name = safe_file_name(&file.path, &torrent.name);
        let dir = self.resolve_download_dir();
        let dest = std::path::Path::new(&dir).join(&name);
        if dest.exists() {
            self.ui.add_log(&format!(
                "{} is already there; not overwriting",
                dest.display()
            ));
            return;
        }
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.ui.add_log(&format!("Could not create {dir}: {e}"));
            return;
        }
        // `/stream` wants 1 for a single-file torrent whatever its id says;
        // for a multi-file one it wants the file's own id.
        let index = if torrent.files.len() == 1 { 1 } else { file.id };
        let torrserver = self.torrserver.clone();
        let hash = torrent.hash.clone();
        let title = torrent.name.clone();
        let path = file.path.clone();
        let shown = dest.display().to_string();
        let event_tx = self.event_handler.sender();
        self.ui
            .add_log(&format!("Saving {name} from {title} to {}", dir));
        tokio::spawn(async move {
            let event = match torrserver.download_file(&hash, index, &path, &dest).await {
                Ok(bytes) => Event::StreamLog(format!(
                    "Saved {shown} ({})",
                    crate::transmission::human_bytes(bytes)
                )),
                Err(e) => Event::StreamLog(format!("Could not save {shown}: {e}")),
            };
            let _ = event_tx.send(event);
        });
    }

    /// Write the torrent's m3u next to the files `s` saves: the same
    /// document the web UI's playlist button downloads, and the one thing
    /// on this panel that is still useful when the server is not running.
    fn save_torrserver_playlist(&mut self, hash: &str, name: &str) {
        let file = playlist_file_name(name);
        let dir = self.resolve_download_dir();
        let dest = std::path::Path::new(&dir).join(&file);
        if dest.exists() {
            self.ui.add_log(&format!(
                "{} is already there; not overwriting",
                dest.display()
            ));
            return;
        }
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.ui.add_log(&format!("Could not create {dir}: {e}"));
            return;
        }
        let torrserver = self.torrserver.clone();
        let hash = hash.to_string();
        let shown = dest.display().to_string();
        let event_tx = self.event_handler.sender();
        self.ui
            .add_log(&format!("Saving the playlist of {name} to {dir}"));
        tokio::spawn(async move {
            let event = match torrserver.playlist(&hash).await {
                Ok(m3u) => match tokio::fs::write(&dest, m3u).await {
                    Ok(()) => Event::StreamLog(format!("Saved playlist {shown}")),
                    Err(e) => Event::StreamLog(format!("Could not write {shown}: {e}")),
                },
                Err(e) => Event::StreamLog(format!("Could not save playlist {shown}: {e}")),
            };
            let _ = event_tx.send(event);
        });
    }

    /// `i`: the server's own row, in a modal. The poster is fetched here
    /// rather than by the renderer because the renderer cannot await -- and
    /// because a modal for a torrent without artwork should not spawn a
    /// request at all.
    fn open_torrserver_details(&mut self, torrent: crate::torrserver::api::TorrentInfo) {
        let poster_url = torrent.poster.trim().to_string();
        let hash = torrent.hash.clone();
        // A picture from the last details page is not this one's.
        self.ui.poster_protocol = None;
        self.ui.modal = Modal::TorrServerDetail(Box::new(TorrServerDetailState::new(torrent)));
        if poster_url.is_empty() {
            return;
        }
        let torrserver = self.torrserver.clone();
        let event_tx = self.event_handler.sender();
        tokio::spawn(async move {
            let poster = torrserver
                .fetch_poster(&poster_url)
                .await
                .map_err(|e| e.to_string());
            let _ = event_tx.send(Event::TorrServerPoster { hash, poster });
        });
    }

    /// Hand a typed magnet or `.torrent` path to TorrServer, the counterpart
    /// of `add_link_to_daemon` for the streaming server.
    pub(super) async fn add_link_to_torrserver(&mut self, link: &str) {
        let title = link_title(link);
        let outcome = if link.trim().to_lowercase().ends_with(".torrent") {
            match std::fs::read(link.trim()) {
                Ok(bytes) => self.torrserver.upload_torrent(&bytes, &title).await,
                Err(e) => {
                    let message = format!("Could not read {link}: {e}");
                    self.ui.add_log(&message.clone());
                    self.ui.magnet_says(message);
                    return;
                }
            }
        } else {
            self.torrserver.add_by_link(link.trim(), &title).await
        };
        match outcome {
            Ok(hash) => {
                self.ui
                    .add_log(&format!("Added {title} to TorrServer ({hash})"));
                self.ui.magnet_done();
            }
            Err(e) => {
                let message = format!("TorrServer refused it: {e}");
                self.ui.add_log(&message.clone());
                self.ui.magnet_says(message);
            }
        }
    }
}

/// The name the playlist is written under: the torrent's own, never a
/// path, and always with the extension a player recognises.
fn playlist_file_name(name: &str) -> String {
    let base = name.trim().replace(['/', '\\'], "_");
    let base = base.trim();
    if base.is_empty() {
        "playlist.m3u".to_string()
    } else {
        format!("{base}.m3u")
    }
}

/// A name safe to write under the download directory: the file's own base
/// name, never a path, and never empty.
fn safe_file_name(path: &str, fallback: &str) -> String {
    let base = std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let base = base.trim();
    if base.is_empty() {
        format!("{}.bin", fallback.trim().replace(['/', '\\'], "_"))
    } else {
        base.to_string()
    }
}

/// The title a link already carries: `dn=` for a magnet, the file's stem
/// for a `.torrent` path, and a neutral word when neither says one.
fn link_title(link: &str) -> String {
    let link = link.trim();
    if link.starts_with("magnet:") {
        if let Some(rest) = link.split("dn=").nth(1) {
            let raw = rest.split('&').next().unwrap_or("");
            let decoded = urlencoding::decode(raw)
                .map(|text| text.into_owned())
                .unwrap_or_else(|_| raw.to_string());
            if !decoded.trim().is_empty() {
                return decoded;
            }
        }
        return "torrent".to_string();
    }
    std::path::Path::new(link)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("torrent")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_path_never_writes_outside_the_directory() {
        assert_eq!(safe_file_name("../../etc/passwd", "fallback"), "passwd");
        assert_eq!(safe_file_name("Movie/movie.mkv", "fallback"), "movie.mkv");
    }

    #[test]
    fn an_empty_path_falls_back_to_the_torrent_name() {
        assert_eq!(safe_file_name("", "Some Torrent"), "Some Torrent.bin");
    }

    #[test]
    fn a_playlist_keeps_the_title_and_never_a_path() {
        assert_eq!(
            playlist_file_name("Остров проклятых (2010)"),
            "Остров проклятых (2010).m3u"
        );
        assert_eq!(playlist_file_name("Show/Season 1"), "Show_Season 1.m3u");
        assert_eq!(playlist_file_name("   "), "playlist.m3u");
    }

    #[test]
    fn a_magnet_title_comes_from_dn() {
        assert_eq!(
            link_title("magnet:?xt=urn:btih:abc&dn=Big%20Buck%20Bunny&tr=udp://x"),
            "Big Buck Bunny"
        );
        assert_eq!(link_title("/tmp/Some.Release.torrent"), "Some.Release");
        assert_eq!(link_title("magnet:?xt=urn:btih:abc"), "torrent");
    }
}
