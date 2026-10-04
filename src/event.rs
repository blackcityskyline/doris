use anyhow::Result;
use crossterm::event::{Event as CrosstermEvent, KeyEvent, MouseEvent};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum Event {
    Tick,
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    /// Text pasted in one piece, which is what a file dragged in from a file
    /// manager is: the terminal sends the path, not the keys for it.
    Paste(String),
    /// One source of `generation` answered: its rows render immediately instead of after the
    /// slowest source.
    SourceDone {
        source: String,
        generation: u64,
        items: Vec<crate::sources::models::TorrentItem>,
        has_more: bool,
        /// Where that source's next page starts, in its own cursor unit; `None` for row-paged
        /// sources and for every failure, both of which leave the cursor where it is.
        next_offset: Option<usize>,
        /// `Some` when the source failed; the message says what happened
        /// (including "timed out after 25s").
        error: Option<String>,
        /// The per-source deadline fired rather than the source's own
        /// error -- reported so status can distinguish the two.
        timed_out: bool,
    },
    /// Every source of `generation` has reported in (or failed to): nothing more will arrive
    /// for it, so the UI may go idle.
    SearchComplete {
        generation: u64,
    },
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
    /// The downloading daemon's torrent list, polled. The Torrents panel's
    /// contents, as opposed to `TorrentListUpdate`, which is the
    /// streaming server's and feeds the streaming status.
    DownloadListUpdate(Vec<crate::ui::view::DownloadRow>),
    /// The downloading daemon did not answer.
    ///
    /// A separate event rather than an empty `DownloadListUpdate` because
    /// an empty list is also what "nothing is downloading" looks like, and
    /// replacing the table with nothing on one failed poll would throw away
    /// every row for a blip. What is being reported is a fact about the
    /// daemon, not about the list.
    DownloadDaemonDown,
    /// A torrent just became the "active" one to show/manage in the Torrent panel (e.g.
    TorrentActive(String),
    /// One download's file list, or why there is not one.
    DownloadFiles {
        id: i64,
        files: Vec<crate::transmission::FileEntry>,
        error: Option<String>,
    },
    /// Hand the terminal to a file manager and take it back afterwards.
    ///
    /// An event rather than a call from the key handler because only the
    /// event loop holds the terminal: the key handler cannot restore what it
    /// did not set up, and a file manager that starts inside the alternate
    /// screen is a file manager nobody can see.
    OpenPath {
        manager: &'static crate::app::files::Manager,
        path: String,
    },
    /// The answer to a detail modal's `Source::details` request: the file list for the page it
    /// was asked about, or the error.
    DetailLoaded {
        page_url: String,
        files: Vec<crate::sources::models::FileEntry>,
        error: Option<String>,
    },
}

pub struct EventHandler {
    tx: tokio::sync::mpsc::UnboundedSender<Event>,
    rx: tokio::sync::mpsc::UnboundedReceiver<Event>,
}

impl EventHandler {
    /// `read_keys` false means "there is no keyboard on the other end":
    /// the thread then only sends ticks. The CLI drives the same handler
    /// the TUI does, and `crossterm::event::poll` on a process with no
    /// terminal fails instantly -- which without this would leave the
    /// thread spinning on a failing poll, sending ticks as fast as the
    /// CPU can manage.
    pub fn new(tick_rate: std::time::Duration, read_keys: bool) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let event_tx = tx.clone();

        std::thread::spawn(move || loop {
            if !read_keys {
                if event_tx.send(Event::Tick).is_err() {
                    break;
                }
                std::thread::sleep(tick_rate);
                continue;
            }
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
                    Ok(CrosstermEvent::Paste(text)) => {
                        if event_tx.send(Event::Paste(text)).is_err() {
                            break;
                        }
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
        });

        Self { tx, rx }
    }

    pub fn sender(&self) -> tokio::sync::mpsc::UnboundedSender<Event> {
        self.tx.clone()
    }

    pub async fn next(&mut self) -> Result<Event> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("Event channel closed"))
    }
}
