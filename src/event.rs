use crossterm::event::{Event as CrosstermEvent, KeyEvent, MouseEvent};
use anyhow::Result;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum Event {
    Tick,
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    /// Results of one search dispatch, tagged with the `search_generation`
    /// of the dispatch that produced them so a late answer from a query
    /// that has since been replaced can be dropped instead of overwriting
    /// the fresh one (B0.2).
    SearchComplete {
        generation: u64,
        results: Vec<crate::search::models::TorrentItem>,
    },
    /// Same generation tag as [`Event::SearchComplete`]: a stale failure
    /// must not flip a newer search back to `Idle` or log an error the user
    /// would attribute to it.
    SearchError {
        generation: u64,
        error: String,
    },
    StreamComplete(String),
    StreamError(String),
    StreamLog(String),
    LoginResult(bool),
    ExtensionQuery(String),
    LoadMore(String, usize),
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
