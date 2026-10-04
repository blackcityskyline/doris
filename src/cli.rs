//! The command line: one subcommand per thing the TUI can do.
//!
//! Every subcommand here is a real action the app already has, reached
//! through the same code the key bindings reach it through -- there is no
//! second implementation of "search" or "play" for the terminal-less case.
//! What differs is only the shape of the answer: a table or a JSON object
//! instead of a panel.
//!
//! Two ways to name a row, because a script has neither a cursor nor a
//! screen: `--magnet` for a URI it already has, and `--query` plus
//! `--index` for one it has to go and fetch. An empty query is browse
//! mode, the same as an empty box in the TUI.

use clap::{Args as ClapArgs, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "doris",
    about = "Search trackers and stream to TorrServer, as a TUI or as a command",
    version
)]
pub struct Args {
    /// Print the answer as JSON. Everything a command can say, it can say
    /// this way, so a pipeline never has to parse the table.
    #[arg(long, global = true)]
    pub json: bool,

    /// The query to search for. With no subcommand this is the TUI's
    /// starting query, exactly as before.
    pub query: Option<String>,

    #[arg(short, long)]
    pub cookie_file: Option<PathBuf>,

    #[arg(short, long)]
    pub username: Option<String>,

    #[arg(short, long)]
    pub password: Option<String>,

    #[arg(short, long)]
    pub browser: Option<String>,

    /// Browser window visibility: "visible" or "hidden" (default: hidden)
    #[arg(long)]
    pub browser_visibility: Option<String>,

    #[arg(long, default_value = crate::torrserver::api::DEFAULT_URL)]
    pub torrserver: String,

    /// The extension bridge listens here. Zero switches it off, which is
    /// what every CLI command does: nothing is going to answer on it.
    #[arg(long, default_value_t = 14141)]
    pub bridge_port: u16,

    #[arg(long)]
    pub config: Option<PathBuf>,

    /// The downloading daemon's RPC address, overriding the config. Not
    /// the streaming one: that is `--torrserver`.
    #[arg(long)]
    pub transmission_url: Option<String>,

    /// Search and print, the way `--cli` always did. Kept because scripts
    /// exist: `doris --cli "query"` is the same as `doris search "query"`.
    #[arg(long, hide = true)]
    pub cli: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Search the checked sources and print the rows.
    Search(SearchArgs),

    /// Send a row to TorrServer and print the URL to watch.
    Play(RowArgs),

    /// Hand a row to the download daemon and print its id there.
    Download(RowArgs),

    /// A row's facts: magnet, page, and the file list its source reads.
    Info(RowArgs),

    /// TorrServer itself: what it is holding, and what to do about it.
    #[command(subcommand)]
    Torrent(TorrentCommand),

    /// Everything the last run logged, newest last.
    Logs,

    /// The source registry: what exists, what is implemented, what is on.
    Sources {
        #[command(subcommand)]
        action: Option<SourcesCommand>,
    },

    /// Browser, TorrServer, credentials, cookies -- one line each.
    Health,

    /// Read and write settings without opening the Options modal.
    #[command(subcommand)]
    Config(ConfigCommand),

    /// Log a resource in and, if "Save credentials" is on, remember it.
    Login(LoginArgs),

    /// The cookie jar the trackers are logged in with.
    #[command(subcommand)]
    Cookies(CookieCommand),

    /// What the downloading daemon holds -- the Torrents panel's list, on
    /// a terminal. This is the one command a script wants that is about
    /// downloads rather than streaming: `torrent` is about TorrServer.
    Downloads(crate::transmission::downloads_cmd::Ask),

    /// The encrypted credential store.
    #[command(subcommand)]
    Credentials(CredentialCommand),
}

#[derive(Subcommand, Debug, Clone)]
pub enum CookieCommand {
    /// Where the jar is and which trackers it has entries for.
    Show,

    /// Delete the jar.
    Clear,
}

#[derive(Subcommand, Debug, Clone)]
pub enum CredentialCommand {
    /// Which resources have a saved login. Never the passwords: they are
    /// in an encrypted file on purpose, and a command line is not the
    /// place to print what that file was for.
    List,

    /// Forget one resource's login.
    Clear { resource: String },
}

/// No subcommand means the listing, which is what this was before it had
/// any.
#[derive(Subcommand, Debug, Clone)]
pub enum SourcesCommand {
    /// What exists, what is implemented, what is on.
    List,

    /// Switch sources on -- what the Trackers panel's checkboxes do.
    On {
        /// Source ids. Repeatable, and comma-separated is fine too.
        ids: Vec<String>,
    },

    /// Switch sources off, leaving the rest as they were.
    Off { ids: Vec<String> },

    /// The order browsers are probed in, for the sources that need one.
    Priority { ids: Vec<String> },
}

#[derive(ClapArgs, Debug, Clone)]
pub struct SearchArgs {
    /// Empty means browse mode: ask for each source's freshest rows.
    pub query: Option<String>,

    /// Ask only this source, by registry id. Repeatable.
    #[arg(long = "source", short = 's')]
    pub source: Vec<String>,

    /// One category: games, movies, tv, anime. `all` means none.
    #[arg(long, short = 'g')]
    pub group: Option<String>,

    /// The Results filter, in the same language the `f` box speaks:
    /// `seeds:>50 src:rutor size:>1gb -archive title:2160`.
    #[arg(long, short = 'f')]
    pub filter: Option<String>,

    /// How many pages to ask for. 1 is a search; 3 is a search and two
    /// "load more"s, which is what the End key does in the TUI.
    #[arg(long, default_value_t = 1)]
    pub pages: usize,

    /// Print at most this many rows after everything is merged and
    /// filtered. No limit means all of them.
    #[arg(long)]
    pub limit: Option<usize>,

    /// Give up on a source that has not answered in this many
    /// milliseconds. Zero waits forever.
    #[arg(long, default_value_t = 60_000)]
    pub deadline_ms: u64,
}

/// How to name the row a command is about.
///
/// `--magnet` names one outright. Otherwise the search flags below find
/// it and `--index` says which: the query lives in [`SearchArgs`] because
/// it is the same query the `search` command takes, and one flag with one
/// meaning is worth more here than a struct that mirrors it.
#[derive(ClapArgs, Debug, Clone)]
pub struct RowArgs {
    /// Which row of the merged, filtered list. Zero is the first.
    #[arg(long, default_value_t = 0)]
    pub index: usize,

    /// Use this magnet URI instead of searching. The clean way to script:
    /// a row chosen once, by hand, and then reused.
    #[arg(long)]
    pub magnet: Option<String>,

    #[command(flatten)]
    pub search: SearchArgs,
}

#[derive(Subcommand, Debug, Clone)]
pub enum TorrentCommand {
    /// Everything TorrServer is holding, as JSON when asked.
    List,

    /// One torrent's live status: progress, speeds, peers.
    Status {
        /// Which one. Omitted means the one doris last started.
        hash: Option<String>,
    },

    /// Stop seeding it; the download keeps its place.
    Pause { hash: Option<String> },

    /// Start a paused torrent again.
    Resume { hash: Option<String> },

    /// Remove it from TorrServer.
    Remove { hash: Option<String> },

    /// Every file the torrent holds, with its size. This is the list the
    /// detail modal shows, read from the same answer.
    Files { hash: Option<String> },

    /// Follow the download until it finishes, the deadline passes, or
    /// Ctrl-C. Prints one line per second.
    Watch {
        hash: Option<String>,
        #[arg(long, default_value_t = 600_000)]
        deadline_ms: u64,
    },

    /// Where the server puts what it fetches, and whether it keeps it.
    #[command(subcommand)]
    Storage(StorageCommand),

    /// Add a magnet or a .torrent URL without searching.
    Add {
        link: String,
        #[arg(long, default_value = "")]
        title: String,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum StorageCommand {
    /// Show the download path and the cache policy.
    Show,

    /// Point the server at a directory and turn disk caching on.
    Set {
        path: std::path::PathBuf,
        /// Also keep the cache after a reader disconnects. Without this
        /// the server drops what it fetched 30 seconds after the last
        /// reader goes away.
        #[arg(long)]
        keep_cache: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub enum ConfigCommand {
    /// Print one setting.
    Get { key: String },

    /// Write one setting and save the file.
    Set { key: String, value: String },

    /// Print every setting, as JSON when asked.
    List,
}

#[derive(ClapArgs, Debug, Clone)]
pub struct LoginArgs {
    /// Which tracker: rutracker, rutor, ...
    pub resource: String,

    #[arg(short, long)]
    pub username: Option<String>,

    #[arg(short, long)]
    pub password: Option<String>,
}

impl Args {
    pub fn from_env() -> Self {
        let mut args = Self::parse();
        if args.username.is_none() {
            args.username = std::env::var("RUTRACKER_USER")
                .or_else(|_| std::env::var("RUTRACKER_USERNAME"))
                .ok();
        }
        if args.password.is_none() {
            args.password = std::env::var("RUTRACKER_PASS")
                .or_else(|_| std::env::var("RUTRACKER_PASSWORD"))
                .ok();
        }
        args
    }

    /// The command to run, with `--cli` folded into `search`.
    ///
    /// `--cli` is a flag that means "search and print", which is what
    /// `search` is called now, so it is translated rather than kept as a
    /// second spelling the rest of the program has to know about.
    pub fn resolve_command(&self) -> Option<Command> {
        match &self.command {
            Some(command) => Some(command.clone()),
            None if self.cli => Some(Command::Search(SearchArgs {
                query: self.query.clone(),
                ..SearchArgs::default()
            })),
            None => None,
        }
    }
}

impl Default for SearchArgs {
    fn default() -> Self {
        Self {
            query: None,
            source: Vec::new(),
            group: None,
            filter: None,
            pages: 1,
            limit: None,
            deadline_ms: 60_000,
        }
    }
}

pub fn parse() -> Args {
    Args::from_env()
}
