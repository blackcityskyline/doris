use crossterm::event::{Event as CrosstermEvent, KeyEvent, MouseEvent};
use anyhow::Result;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum Event {
    Tick,
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    /// One source of `generation` answered: its rows render immediately
    /// instead of after the slowest source (B3). Tagged with the
    /// `search_generation` of the dispatch that produced it, so a late
    /// answer from a query that has since been replaced is dropped
    /// instead of overwriting the fresh one (B0.2).
    SourceDone {
        source: String,
        generation: u64,
        items: Vec<crate::search::models::TorrentItem>,
        /// Whether *that* source has another page (B2). `App` remembers
        /// it per source so "Load more" only asks the ones that do.
        has_more: bool,
        /// Where that source's next page starts, in its own cursor unit;
        /// `None` for row-paged sources and for every failure, both of
        /// which leave the cursor where it is. See
        /// `SearchPage::next_offset`.
        next_offset: Option<usize>,
        /// `Some` when the source failed; the message says what happened
        /// (including "timed out after 25s").
        error: Option<String>,
        /// The per-source deadline fired rather than the source's own
        /// error -- reported so status can distinguish the two.
        timed_out: bool,
    },
    /// Every source of `generation` has reported in (or failed to):
    /// nothing more will arrive for it, so the UI may go idle. Rows
    /// already arrived individually via [`Event::SourceDone`].
    SearchComplete { generation: u64 },
    StreamComplete(String),
    StreamError(String),
    StreamLog(String),
    LoginResult(bool),
    ExtensionQuery(String),
    /// Latest full torrent list from TorrServer's poller
    /// (`torrent::Manager`), sent on every poll tick regardless of whether
    /// anything changed -- the receiver decides what (if anything) to
    /// update.
    TorrentListUpdate(Vec<crate::torrserver::api::TorrentInfo>),
    /// A torrent just became the "active" one to show/manage in the
    /// Torrent panel (e.g. right after it was uploaded to TorrServer for
    /// streaming).
    TorrentActive(String),
}

pub struct EventHandler {
    tx: tokio::sync::mpsc::UnboundedSender<Event>,
    rx: tokio::sync::mpsc::UnboundedReceiver<Event>,
}

impl EventHandler {
    pub fn new(tick_rate: std::time::Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let event_tx = tx.clone();

        std::thread::spawn(move || {
            loop {
                if crossterm::event::poll(tick_rate).unwrap_or(false) {
                    match crossterm::event::read() {
                        Ok(CrosstermEvent::Key(key)) => {
                            if event_tx.send(Event::Key(key)).is_err() {
                                break;
                            }
                        }
                        Ok(CrosstermEvent::Mouse(mouse)) => {
                            let _ = event_tx.send(Event::Mouse(mouse));
                        }
                        Ok(CrosstermEvent::Resize(w, h)) => {
                            let _ = event_tx.send(Event::Resize(w, h));
                        }
                        _ => {}
                    }
                } else {
                    if event_tx.send(Event::Tick).is_err() {
                        break;
                    }
                }
            }
        });

        Self { tx, rx }
    }

    pub fn sender(&self) -> tokio::sync::mpsc::UnboundedSender<Event> {
        self.tx.clone()
    }

    pub async fn next(&mut self) -> Result<Event> {
        self.rx.recv().await.ok_or_else(|| anyhow::anyhow!("Event channel closed"))
    }
}
