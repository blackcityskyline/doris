//! Background polling for the downloading daemon's torrent list.
//!
//! The twin of [`crate::torrent::Manager`], which does the same for
//! TorrServer. They exist for the same reason: a panel that is written
//! once at startup and never again is not a live panel, it is a
//! screenshot. TorrServer's list feeds the streaming side and this one's
//! feeds the Torrents panel.
//!
//! Failure is silent and rate-limited. A daemon that is not running must
//! not turn into an event every second, and a log line every second is
//! how a log becomes unreadable.

use crate::event::Event;
use crate::transmission::Transmission;
use tokio::sync::mpsc::UnboundedSender;

/// The shortest gap between polls, whatever the config asks for.
pub const MIN_POLL_INTERVAL_MS: u64 = 100;

/// How long a daemon that is not answering is left alone before it is
/// tried again. Long enough that a stopped daemon costs nothing, short
/// enough that starting it does not need a restart of doris to notice.
const RETRY_BACKOFF_MS: u64 = 15_000;

pub struct Poller;

impl Poller {
    /// Spawn a task that polls `transmission.list()` every `update_ms`
    /// and forwards each answer as `Event::DownloadListUpdate`.
    pub fn spawn(
        transmission: Transmission,
        update_ms: u64,
        event_tx: UnboundedSender<Event>,
    ) -> tokio::task::JoinHandle<()> {
        let period = std::time::Duration::from_millis(update_ms.max(MIN_POLL_INTERVAL_MS));
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(period);
            let mut failures = 0u32;
            loop {
                interval.tick().await;
                // Back off in steps of the poll period, so a daemon that
                // is down is not asked again on the next tick.
                if failures > 0 {
                    let waited = failures as u64 * RETRY_BACKOFF_MS;
                    let mut backoff =
                        tokio::time::interval(std::time::Duration::from_millis(waited));
                    backoff.tick().await;
                    backoff.tick().await;
                    failures = failures.saturating_sub(1);
                }
                match transmission.list().await {
                    Ok(rows) => {
                        failures = 0;
                        if event_tx
                            .send(Event::DownloadListUpdate(
                                rows.iter()
                                    .map(crate::transmission::adopt::to_row)
                                    .collect(),
                            ))
                            .is_err()
                        {
                            // Receiver gone -- app is shutting down.
                            break;
                        }
                    }
                    Err(_) => failures = failures.saturating_add(1),
                }
            }
        })
    }
}
