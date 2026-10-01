//! Background polling for live torrent status. It exists because the Torrent panel used to be
//! permanently frozen at its default values: nothing ever polled TorrServer for status.

use crate::event::Event;
use crate::torrserver::api::TorrServer;
use tokio::sync::mpsc::UnboundedSender;

/// Minimum interval between TorrServer polls.
const MIN_POLL_INTERVAL_MS: u64 = 100;

pub struct Manager;

impl Manager {
    /// Spawn a task that polls `torrserver.list_torrents()` every `update_ms` and forwards the
    /// result as `Event::TorrentListUpdate`.
    pub fn spawn(
        torrserver: TorrServer,
        update_ms: u64,
        event_tx: UnboundedSender<Event>,
    ) -> tokio::task::JoinHandle<()> {
        let period = std::time::Duration::from_millis(update_ms.max(MIN_POLL_INTERVAL_MS));
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
