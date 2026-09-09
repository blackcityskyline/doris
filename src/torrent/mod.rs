//! Background polling for live torrent status (ROADMAP.md Phase 7, fixes
//! bug B3: the Torrent panel used to be permanently frozen at its default
//! values because nothing ever polled TorrServer for status).
//!
//! `Manager` owns nothing but the spawned task itself -- the actual
//! "current status" state lives on `ui::App` (`torrent_status`,
//! `active_torrent_hash`), updated by the orchestrator whenever a
//! [`crate::event::Event::TorrentListUpdate`] arrives. This keeps the
//! polling concern (how often, how to fetch) separate from the
//! presentation concern (what to show), so a future second poller -- e.g.
//! for a local (non-TorrServer) download engine -- can post to the same
//! event without either side knowing about the other.

use crate::event::Event;
use crate::torrserver::api::TorrServer;
use tokio::sync::mpsc::UnboundedSender;

pub struct Manager;

impl Manager {
    /// Spawn a task that polls `torrserver.list_torrents()` every
    /// `update_ms` and forwards the result as `Event::TorrentListUpdate`.
    ///
    /// The interval is fixed for the lifetime of the task: changing
    /// "Update ms" in Options takes effect on the next restart, not live.
    /// Making it live would mean sharing the value through something like
    /// an `Arc<AtomicU64>` instead of a plain `Config` field -- a
    /// reasonable follow-up, not done here to keep this first version
    /// simple.
    ///
    /// A TorrServer that's unreachable just means quiet ticks (no event
    /// sent) rather than an error -- TorrServer commonly isn't running
    /// until the user actually starts a stream, and that's not a problem
    /// worth logging on every poll.
    pub fn spawn(torrserver: TorrServer, update_ms: u64, event_tx: UnboundedSender<Event>) -> tokio::task::JoinHandle<()> {
        let period = std::time::Duration::from_millis(update_ms.max(100));
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(period);
            loop {
                interval.tick().await;
                if let Ok(list) = torrserver.list_torrents().await {
                    if event_tx.send(Event::TorrentListUpdate(list)).is_err() {
                        // Receiver gone -- app is shutting down.
                        break;
                    }
                }
            }
        })
    }
}
