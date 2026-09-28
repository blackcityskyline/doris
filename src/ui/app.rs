use super::menu::MenuState;
use super::theme::Theme;
use super::zones::{FrameButton, FrameSlot, ZoneId, ZoneLayout, SEARCH_BAR_HEIGHT};
use crate::config::Config;
use crate::sources::models::{FileEntry, TorrentItem};
use crate::sources::orchestrator::SourceStatus;
use crate::sources::source::{Group, KNOWN_SOURCES};
use crate::ui::modals::help::HelpState;
use crate::ui::modals::login::LoginState;
use crate::ui::modals::settings::{group_tabs, SettingsState};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;
use std::collections::{HashMap, VecDeque};

/// Where the search is. Failures deliberately have no variant: an error
/// belongs to one source, not to the whole app, so it lives in
/// [`App::source_status`] (drawn in the Sources panel) and in the log's
/// per-source outcome line -- one source failing never stops the ones
/// that answered, and the panel must not claim otherwise.
#[derive(PartialEq)]
pub enum AppState {
    Idle,
    Searching,
    Streaming,
}

/// What a frame-button click (or the equivalent key) needs the
/// orchestrator to do.
///
/// Buttons whose effect `ui::App` can perform itself -- filter, group,
/// source -- are handled inside `click_at` and never surface as an
/// `UiAction`; these are the ones that reach outside the UI state (an
/// async TorrServer call, a search restart, the results list).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    Play,
    Download,
    Info,
    TogglePause,
    Remove,
}

#[derive(PartialEq, Clone, Debug)]
pub enum Modal {
    None,
    Login(LoginState),
    Settings(SettingsState),
    HealthCheck(Vec<String>),
    Help(HelpState),
    /// The selected row's details (П.7): the row itself, the file list
    /// its source is still fetching (or has fetched), and the cursor
    /// into that list.
    ///
    /// Boxed because the state carries a whole `TorrentItem`: an enum
    /// variant that big would make every `Modal` -- including the
    /// `HealthCheck(Vec<String>)` that is just a few lines -- pay for it.
    TorrentDetail(Box<TorrentDetailState>),
}

/// What a key in the detail modal asks the orchestrator for. The modal
/// owns the cursor and the list; playing and downloading are the
/// orchestrator's, exactly like a frame button's [`UiAction`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailAction {
    Play,
    Download,
}

/// The detail modal's state (П.7).
#[derive(Clone, Debug, PartialEq)]
pub struct TorrentDetailState {
    /// The row the modal was opened from. Its facts are on screen before
    /// anything is fetched, so the modal is never an empty box.
    pub item: TorrentItem,
    /// The files inside the torrent, once `Source::details` answered.
    pub files: Vec<FileEntry>,
    /// `true` while that answer is in flight: the modal says so rather
    /// than leaving a blank list that reads as "no files".
    pub pending: bool,
    /// Why the file list could not be read, when it could not.
    pub error: Option<String>,
    /// Which file the cursor is on.
    pub cursor: usize,
}

impl TorrentDetailState {
    pub fn new(item: TorrentItem) -> Self {
        Self {
            item,
            files: Vec::new(),
            pending: true,
            error: None,
            cursor: 0,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TorrentStatus {
    pub hash: String,
    pub title: String,
    pub progress: f64,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub seeds: u32,
    pub peers: u32,
    pub downloaded: u64,
    pub total_size: u64,
    pub status: String,
}

pub struct App {
    pub search_input: String,
    pub results: Vec<TorrentItem>,
    pub selected: usize,
    pub logs: VecDeque<String>,
    pub log_scroll: usize,
    pub detail_logs: Vec<String>,
    pub detail_log_mode: bool,
    pub detail_log_scroll: usize,
    pub state: AppState,
    /// What each source answered for the running search: pending, how
    /// many rows, an error or a deadline. Lives here because this is
    /// the only place that ever shows it -- the Sources panel reads it
    /// per row.
    pub source_status: HashMap<String, SourceStatus>,
    pub torrserver_url: String,
    pub running: bool,
    pub input_mode: bool,
    pub modal: Modal,
    pub search_query: Option<String>,
    pub all_loaded: bool,
    pub stream_mode: bool,
    pub theme: Theme,
    pub zones: ZoneLayout,
    pub menu: MenuState,
    pub show_menu: bool,
    pub torrent_status: TorrentStatus,
    /// Hash of the torrent the Torrent panel currently shows/manages.
    /// `None` means "show whatever TorrServer reports first" (see
    /// app.rs's TorrentListUpdate handler); set once a stream is started
    /// via spawn_stream so pause/resume/remove act on the right torrent
    /// even if others are also active.
    pub active_torrent_hash: Option<String>,
    /// Client-side pause tracking. TorrServer has no "paused" torrent
    /// state to read back -- pausing means `drop`ping the torrent, which
    /// typically removes it from the live list entirely rather than
    /// reporting it as paused -- so this is the source of truth for what
    /// the 'p' key should do next, not something derived from polling.
    pub torrent_paused: bool,
    /// Which row of the Sources panel the cursor sits on: 0 is the `all`
    /// master switch, 1.. the registry entries. The panel is the only
    /// place sources are switched (П.4), so this is the only cursor the
    /// enabled set has.
    pub sources_cursor: usize,
    /// Which category the Results table is showing -- the tab row under
    /// the frame; `None` is the "all" tab. Search dispatch in app.rs
    /// reads this to fill `SearchRequest.category` and to skip the
    /// sources that do not serve it (B6).
    pub active_group: Option<Group>,
    /// The category row itself: "all", then every group at least one
    /// enabled, implemented source serves, in `GROUP_ORDER`. Held rather
    /// than computed per frame so the drawn row, the hit-test and the
    /// selection can never disagree -- and so a group nothing can answer
    /// is never drawn, the same reasoning wave 1 settled on for disabled
    /// sources.
    pub group_tabs: Vec<Option<Group>>,
    /// Set when the category is switched (`g`/`G` or a click), cleared by
    /// the next `start_search`: Enter means "re-search with the new
    /// selection" for this row too. The other half of a switch happens
    /// right there in [`UiApp::set_group`] -- the view is re-derived from
    /// the rows already on screen -- so nothing here implies a request
    /// was already made.
    pub group_changed: bool,
    /// Browse mode (B9): the current search is an empty query asking
    /// browse-capable sources for their freshest rows. Set by the `b`
    /// key, which also returns the category to "all" -- a browse list is
    /// mixed by nature, so rows claiming no group must stay visible.
    pub browsing: bool,
    /// Set whenever the user switches the active source tab (via `]` key or
    /// mouse click). Cleared on the next Enter press, which uses it to
    /// decide whether Enter means "re-search with the new source" (true)
    /// or "play the selected torrent" (false).
    pub source_changed: bool,
    /// Set by `settings_key` right before it returns a cycle-type
    /// SettingsAction (CycleTheme/CyclePreset/etc): +1 for Right/Enter,
    /// -1 for Left. The orchestrator's handler for that action reads this
    /// to decide which direction to step -- SettingsAction itself has no
    /// payload, so this is how Left and Right stop being identical
    /// (previously both always cycled forward).
    pub last_cycle_direction: i8,
    /// Rolling progress history feeding the Torrent panel's sparkline
    /// (ROADMAP.md Phase 8). Oldest first; capped in app.rs's
    /// TorrentListUpdate handler so a long session doesn't grow this
    /// unboundedly.
    pub progress_history: std::collections::VecDeque<f64>,
    /// Session copy of the runtime browser visibility, refreshed from
    /// `App::browser_visibility` every time the settings modal opens. The
    /// modal displays and toggles it; the orchestrator owns the value.
    pub settings_browser_hidden: bool,
    pub filtered_indices: Vec<usize>,
}

/// Width of the `Src` column in the results table. Fixed on purpose:
/// the longest source id in `KNOWN_SOURCES` is 10 characters, so the
/// columns never shift as the results change -- see
/// `test_every_known_source_fits_the_badge_column`.
pub const SOURCE_BADGE_WIDTH: u16 = 10;

/// The `Src` cell for one result: the source id, or `-` when it is
/// missing.
///
/// `TorrentItem::source` is `#[serde(default)]`, so rows persisted
/// before the field existed (or produced by a path that never filled it
/// in) arrive empty. A blank cell would read as "the column is empty
/// here" rather than "nobody knows", hence the explicit placeholder.
///
/// Most useful on the `all` tab, where a single page mixes results from
/// several trackers and the row itself is the only place that says who
/// returned it.
pub fn source_badge(item: &TorrentItem) -> String {
    if item.source.is_empty() {
        "-".to_string()
    } else {
        item.source.clone()
    }
}

/// One row of the Sources panel: the `all` switch, then the registry in
/// order (П.4). A row past the end is `None`, so the cursor and the
/// hit-test share one list to walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRow {
    /// "Ask every checked source" -- the default view, and the row that
    /// checks or unchecks the whole roster in one keypress.
    All,
    /// One registered source, by id.
    One(&'static str),
}

impl SourceRow {
    /// What the row says on screen. `all` is the master switch, so it
    /// carries the same `[x]`/`[ ]` as the sources below it.
    pub fn id(&self) -> &'static str {
        match self {
            SourceRow::All => "all",
            SourceRow::One(id) => id,
        }
    }

    /// Whether this source can be switched on at all: an unimplemented
    /// entry is listed (so the next source is visible where it will
    /// land) but Enter on it does nothing.
    pub fn is_implemented(&self) -> bool {
        match self {
            SourceRow::All => true,
            SourceRow::One(id) => KNOWN_SOURCES
                .iter()
                .find(|info| info.id == *id)
                .map(|info| info.implemented)
                .unwrap_or(false),
        }
    }

    /// The checkbox state this row draws, read from the config the panel
    /// edits. `all` is checked when *every* implemented source is -- with
    /// none implemented it reads as off rather than claiming a default
    /// nobody set.
    pub fn is_checked(&self, config: &Config) -> bool {
        match self {
            SourceRow::All => {
                let ids: Vec<&'static str> = KNOWN_SOURCES
                    .iter()
                    .filter(|info| info.implemented)
                    .map(|info| info.id)
                    .collect();
                !ids.is_empty()
                    && ids
                        .iter()
                        .all(|id| config.enabled_sources.iter().any(|e| e == id))
            }
            SourceRow::One(id) => config.enabled_sources.iter().any(|e| e == id),
        }
    }
}

/// The panel's rows in draw order: `all`, then every registered source
/// in registry order -- the same order the registry itself is read in
/// everywhere else, so a new entry lands here on its own.
pub fn source_rows() -> Vec<SourceRow> {
    let mut rows = vec![SourceRow::All];
    rows.extend(KNOWN_SOURCES.iter().map(|info| SourceRow::One(info.id)));
    rows
}

/// The row at `index`, if the panel has one.
pub fn source_row_at(index: usize) -> Option<SourceRow> {
    source_rows().into_iter().nth(index)
}

/// How much of a source's error text a Sources row shows. An error is a
/// sentence, and a sentence does not fit on a one-line row.
const STATUS_TEXT_WIDTH: usize = 24;

/// What a Sources row appends after its checkbox: what that source
/// answered for the search that ran. Without it the panel said only
/// which sources were *asked*, never which of them replied.
fn source_status_text(status: &SourceStatus) -> String {
    match status {
        SourceStatus::Pending => "…".to_string(),
        SourceStatus::Ok(rows) => format!("✓ {rows}"),
        SourceStatus::Timeout => "✗ timeout".to_string(),
        SourceStatus::Error(err) => {
            let clipped: String = err.chars().take(STATUS_TEXT_WIDTH).collect();
            if err.chars().count() > STATUS_TEXT_WIDTH {
                format!("✗ {clipped}…")
            } else {
                format!("✗ {err}")
            }
        }
    }
}

/// The status's own colour, so a failure reads at a glance: dim while
/// still in flight, the informational mid-bright for an answer, red for
/// a refusal. The label says it in words too, so colour is never the
/// only channel carrying the meaning.
fn source_status_style(status: &SourceStatus, theme: &Theme) -> Style {
    match status {
        SourceStatus::Pending => Style::default().fg(theme.inactive_fg.to_color()),
        SourceStatus::Ok(_) => Style::default().fg(theme.graph_text.to_color()),
        SourceStatus::Timeout | SourceStatus::Error(_) => Style::default().fg(Color::Red),
    }
}

/// What the Results frame's info slot says the search is asking:
/// `all` when every implemented source is checked (the default view),
/// `none` when nothing is, otherwise the checked ids in registry order.
///
/// Short because that slot shares the top border with the zone title,
/// the frame buttons and the row counts -- and anything that does not fit
/// is dropped rather than clipped (П.5), so a long list must not be the
/// only way to say what is on.
pub fn sources_summary(config: &Config) -> String {
    let ids: Vec<&'static str> = KNOWN_SOURCES
        .iter()
        .filter(|info| info.implemented)
        .filter(|info| config.enabled_sources.iter().any(|e| e == info.id))
        .map(|info| info.id)
        .collect();
    let implemented = KNOWN_SOURCES.iter().filter(|info| info.implemented).count();
    if ids.is_empty() {
        "none".to_string()
    } else if ids.len() == implemented {
        "all".to_string()
    } else {
        ids.join(", ")
    }
}

/// Columns kept between two elements of a frame legend; btop's buttons
/// sit a couple of columns apart on the border, not flush against each
/// other.
const FRAME_GAP: u16 = 2;

/// The rects [`App::frame_layout`] hands out: every frame button with
/// the screen rectangle it is drawn into, plus the panel's info text.
#[derive(Debug, Default, Clone)]
pub struct FrameLayout {
    /// Every clickable button of the zone, in draw order.
    pub buttons: Vec<(FrameButton, Rect)>,
    /// Zero-sized when the zone shows no info text.
    pub info: Rect,
    pub info_text: String,
}

impl FrameLayout {
    /// Which button (and the rect it occupies), if any, is under
    /// `(col, row)`. The rect comes back with it because the category
    /// button's two arrow cells are separate targets, and telling them
    /// apart needs to know where the click landed inside the button.
    pub fn button_at(&self, col: u16, row: u16) -> Option<(FrameButton, Rect)> {
        let pos = ratatui::layout::Position::new(col, row);
        self.buttons
            .iter()
            .find(|(_, r)| r.contains(pos))
            .map(|(b, r)| (b.clone(), *r))
    }
}

impl App {
    pub fn new(torrserver_url: String, theme_name: Option<&str>) -> Self {
        let theme = theme_name
            .and_then(|name| Theme::load_themes().into_iter().find(|t| t.name == name))
            .unwrap_or_default();

        Self {
            search_input: String::new(),
            results: Vec::new(),
            selected: 0,
            logs: VecDeque::new(),
            log_scroll: 0,
            detail_logs: Vec::new(),
            detail_log_mode: false,
            detail_log_scroll: 0,
            state: AppState::Idle,
            source_status: HashMap::new(),
            torrserver_url,
            running: true,
            input_mode: false,
            modal: Modal::None,
            search_query: None,
            all_loaded: false,
            stream_mode: true,
            theme,
            zones: ZoneLayout::new(),
            menu: MenuState::new(),
            show_menu: false,
            torrent_status: TorrentStatus::default(),
            active_torrent_hash: None,
            torrent_paused: false,
            // Which row of the Sources panel the cursor sits on: 0 is
            // the `all` switch, 1.. the registry entries.
            sources_cursor: 0,
            active_group: None,
            // A fresh install's view: every implemented source is on by
            // default. `App::new` immediately re-derives it from the
            // config actually being loaded.
            group_tabs: group_tabs(&Config::default()),
            group_changed: false,
            browsing: false,
            source_changed: false,
            last_cycle_direction: 1,
            progress_history: std::collections::VecDeque::new(),
            settings_browser_hidden: false,
            filtered_indices: Vec::new(),
        }
    }

    pub fn add_log(&mut self, msg: &str) {
        let ts = chrono::Local::now().format("%H:%M:%S").to_string();
        // Follow the reader, not the writer: a new line only drags the
        // panel down if it was already parked at the bottom. A reader
        // halfway up a history keeps the history they were reading.
        let following = self.log_scroll >= self.logs.len();
        self.logs.push_back(format!("[{}] {}", ts, msg));
        if self.logs.len() > 500 {
            self.logs.pop_front();
            if !following {
                self.log_scroll = self.log_scroll.saturating_sub(1);
            }
        }
        if following {
            self.log_scroll = self.logs.len();
        }
    }

    pub fn add_detail(&mut self, msg: &str) {
        let ts = chrono::Local::now().format("%H:%M:%S%.3f").to_string();
        let following = self.detail_log_scroll >= self.detail_logs.len();
        self.detail_logs.push(format!("[{}] {}", ts, msg));
        if following {
            self.detail_log_scroll = self.detail_logs.len();
        }
    }

    pub fn toggle_detail_log(&mut self) {
        self.detail_log_mode = !self.detail_log_mode;
    }

    pub fn scroll_logs_up(&mut self) {
        self.log_scroll = self.log_scroll.saturating_sub(1);
    }

    pub fn scroll_logs_down(&mut self) {
        self.log_scroll = (self.log_scroll + 1).min(self.logs.len());
    }

    pub fn scroll_logs_page_up(&mut self) {
        self.log_scroll = self.log_scroll.saturating_sub(10);
    }

    pub fn scroll_logs_page_down(&mut self) {
        self.log_scroll = (self.log_scroll + 10).min(self.logs.len());
    }

    pub fn mouse_scroll_logs(&mut self, delta: i16) {
        if delta > 0 {
            for _ in 0..delta {
                self.scroll_logs_up();
            }
        } else {
            for _ in 0..(-delta) {
                self.scroll_logs_down();
            }
        }
    }

    /// Which zone (if any) contains screen position `(row, col)`, honoring
    /// fullscreen mode (only the fullscreened zone is hit-testable while
    /// active). Shared by mouse clicks (`click_at`) and scroll-wheel
    /// hover-targeting in the orchestrator, so "click a panel" and "scroll
    /// over a panel" agree on which panel that is.
    /// Switch the category row to `group` -- the single path behind both
    /// `g`/`G` and a click on the row.
    ///
    /// Two halves of the same decision: the view is re-derived from the
    /// rows already on screen, so "Movies" means Movies *now* and not
    /// after the next search; and `group_changed` tells Enter that the
    /// sources still owe the server-side answer. Neither half makes a
    /// request on its own -- one keypress stays one keypress, and the
    /// search that follows is the Enter that was always required after
    /// switching a tab.
    pub fn set_group(&mut self, group: Option<Group>) {
        if self.active_group == group {
            return;
        }
        self.active_group = group;
        self.group_changed = true;
        self.update_filter();
    }

    /// Move the category row one tab, forward for `g` and back for `G`
    /// (wraps). The row always holds "all", so the modulo is safe even
    /// with every source switched off -- same reasoning as
    /// [`UiApp::cycle_source`].
    pub fn cycle_group(&mut self, forward: bool) {
        let pos = self
            .group_tabs
            .iter()
            .position(|&g| g == self.active_group)
            .unwrap_or(0);
        let len = self.group_tabs.len();
        let next = if forward {
            (pos + 1) % len
        } else {
            (pos + len - 1) % len
        };
        self.set_group(self.group_tabs[next]);
    }

    /// [`UiApp::new`] takes no config, so the caller that *does* have
    /// one applies the rows it implies; chaining keeps that from being
    /// an easy line to forget at construction.
    pub fn with_group_tabs(mut self, config: &Config) -> Self {
        self.set_group_tabs(config);
        self
    }

    /// Re-derive the category row from `config` and repair
    /// `active_group` when the tab it pointed at has just disappeared (a
    /// source switched off in the Sources panel can take a group with it
    /// when no other enabled source serves it).
    ///
    /// Called once from `App::new` and after every enable/disable --
    /// the two moments the enabled set changes. A *valid* tab is left
    /// alone, so opening and closing Settings never disturbs where the
    /// user already was.
    pub fn set_group_tabs(&mut self, config: &Config) {
        self.group_tabs = group_tabs(config);
        if !self.group_tabs.contains(&self.active_group) {
            // "all" is always first, so this is `Some(None)` by
            // construction; `flatten` is only here because
            // `first().copied()` on `Vec<Option<Group>>` keeps both
            // layers of the Option.
            self.active_group = self.group_tabs.first().copied().flatten();
        }
    }

    /// Move the Sources panel's cursor by `delta` rows, wrapping both ways
    /// -- btop wraps its lists too, so the panel never dead-ends.
    pub fn navigate_sources(&mut self, delta: i64) {
        let len = source_rows().len() as i64;
        if len == 0 {
            return;
        }
        let next = (self.sources_cursor as i64 + delta).rem_euclid(len);
        self.sources_cursor = next as usize;
    }

    /// The row of the Sources panel under `(row, col)`, given that zone's
    /// current area: the top border, then one row per entry of
    /// [`source_rows`], starting one column in. Kept in lockstep with
    /// render_sources_zone's own layout by construction -- both are one
    /// row below the top border and start one column after the left
    /// border, the same convention `frame_layout` uses for the buttons it
    /// hangs there.
    pub fn sources_row_at(&self, row: u16, _col: u16) -> Option<SourceRow> {
        let area = self.zones.get_area(ZoneId::Sources);
        if area.width == 0 || area.height == 0 {
            return None;
        }
        let line = row.checked_sub(area.y)?;
        // -1 for the panel border: line 0 inside the box is the first row.
        let index = line.checked_sub(1)? as usize;
        source_row_at(index)
    }

    /// Switch the row under the cursor in `config`, then re-derive the
    /// category row (a source switched off can take a group with it) and
    /// tell Enter that the enabled set owes a search.
    ///
    /// The `all` row is the master switch: checked, it checks the whole
    /// roster; unchecked, it clears it. A source that is not implemented
    /// is left alone -- toggling something that cannot run would be a lie
    /// in the other direction (the same rule the old Options rows had).
    pub fn toggle_source(&mut self, config: &mut Config) {
        let before = config.enabled_sources.clone();
        match source_row_at(self.sources_cursor) {
            Some(SourceRow::All) => {
                let all_on = SourceRow::All.is_checked(config);
                if all_on {
                    config.enabled_sources.clear();
                } else {
                    config.enabled_sources = KNOWN_SOURCES
                        .iter()
                        .filter(|info| info.implemented)
                        .map(|info| info.id.to_string())
                        .collect();
                }
            }
            Some(SourceRow::One(id)) => {
                if !SourceRow::One(id).is_implemented() {
                    return;
                }
                if config.enabled_sources.iter().any(|s| s == id) {
                    config.enabled_sources.retain(|s| s != id);
                } else {
                    config.enabled_sources.push(id.to_string());
                }
            }
            None => {}
        }
        // Only a real change owes a search: a click on a row that did
        // nothing (a planned source, a cursor past the end) must not
        // make the next Enter re-run the query.
        if config.enabled_sources != before {
            self.set_group_tabs(config);
            self.source_changed = true;
        }
    }

    pub fn zone_at(&self, row: u16, col: u16) -> Option<ZoneId> {
        for &id in ZoneId::all() {
            if let Some(fs) = self.zones.fullscreen {
                if fs != id {
                    continue;
                }
            }
            let area = self.zones.get_area(id);
            if area.width == 0 || area.height == 0 {
                continue;
            }
            let inside = row >= area.y
                && row < area.y + area.height
                && col >= area.x
                && col < area.x + area.width;
            if inside {
                return Some(id);
            }
        }
        None
    }

    /// The text a zone shows next to its frame buttons: the sources the
    /// search asks and the row counts for Results, the checked count for
    /// Sources, the scroll position for Log.
    ///
    /// One function so the renderer and [`App::frame_layout`] always
    /// agree on how wide it is -- `frame_layout` is what `click_at` hits
    /// against, so a width that differed between the two would make the
    /// legend drawn and the legend clickable two different things.
    pub fn frame_info(&self, id: ZoneId, area: Rect, config: &Config) -> String {
        match id {
            ZoneId::Results => {
                let counts = format!(" ({}/{})", self.filtered_indices.len(), self.results.len());
                let sources = format!("[{}]", sources_summary(config));
                if self.zones.filter_input.is_empty() {
                    format!("{} {}", sources, counts)
                } else {
                    format!(" [F: {}] {} {}", self.zones.filter_input, sources, counts)
                }
            }
            ZoneId::Sources => {
                let checked = config
                    .enabled_sources
                    .iter()
                    .filter(|id| {
                        KNOWN_SOURCES
                            .iter()
                            .any(|info| info.implemented && info.id == **id)
                    })
                    .count();
                let total = KNOWN_SOURCES.iter().filter(|info| info.implemented).count();
                format!(" ({}/{})", checked, total)
            }
            ZoneId::Log => {
                let total = self.logs.len();
                let visible = (area.height as usize).saturating_sub(2);
                let offset = self.log_scroll.saturating_sub(visible);
                if total > 0 {
                    format!(" ({}/{})", offset + visible.min(total), total)
                } else {
                    String::new()
                }
            }
            ZoneId::Torrent | ZoneId::Extra => String::new(),
        }
    }

    /// Screen rects for `id`'s frame buttons and info text.
    ///
    /// Single source of truth: the renderer draws into exactly these
    /// rects and `click_at` tests exactly these rects, so what is drawn
    /// on the border is what a click hits. btop pairs them the same way
    /// -- each span written in `btop_draw.cpp` is immediately followed by
    /// the `Input::mouse_mappings[...]` line covering those columns.
    ///
    /// Anything that does not fit is dropped rather than clipped: the
    /// top-right cluster disappears when the zone gets narrow, which is
    /// btop's `if (width > 60 + sort_len)` guard in rect form.
    pub fn frame_layout(&self, id: ZoneId, area: Rect, config: &Config) -> FrameLayout {
        let mut out = FrameLayout::default();
        // Two border columns plus somewhere to put something: shorter or
        // narrower than this there is no legend to draw.
        if area.width < 6 || area.height < 3 {
            return out;
        }

        let left = area.x + 1; // first column inside the border
        let right = area.x + area.width - 2; // last one before the corner
        let top = area.y;
        let bottom = area.y + area.height - 1;
        let fits = |x: u16, w: u16| w > 0 && x <= right && x + w - 1 <= right;

        let mut buttons = super::zones::zone_buttons(id);
        out.info_text = self.frame_info(id, area, config);
        let info_width = out.info_text.chars().count() as u16;

        // The category button is built here rather than in the static
        // table because its label names the current category: btop's
        // `◀ name ▶` sort header, with the two arrows as mouse targets
        // (previous / next category). The `g`/`G` keys stay the
        // keyboard way in, exactly as they were when the category was a
        // row inside the panel.
        //
        // The name is padded to the widest category so the arrows stay
        // in the same columns no matter which one is showing -- `◀ TV ▶`
        // and `◀ Movies ▶` line up, instead of the right arrow sliding
        // four columns to the right on the longer name.
        if id == ZoneId::Results {
            let width = self
                .group_tabs
                .iter()
                .map(|g| g.map_or("all", Group::label).chars().count())
                .max()
                .unwrap_or(3);
            let name = self.active_group.map_or("all", Group::label);
            buttons.push(FrameButton {
                slot: FrameSlot::TopRight,
                key: 'g',
                label: format!("◀ {:<width$} ▶", name, width = width),
            });
        }

        // Top left: the title already claims `zone_title_width` columns
        // after the border, then the buttons, then the info text.
        let mut x = left + super::zones::zone_title_width(id);
        for b in buttons.iter().filter(|b| b.slot == FrameSlot::TopLeft) {
            if !fits(x, b.width()) {
                break;
            }
            out.buttons
                .push((b.clone(), Rect::new(x, top, b.width(), 1)));
            x += b.width() + FRAME_GAP;
        }
        if fits(x, info_width) {
            out.info = Rect::new(x, top, info_width, 1);
            x += info_width + FRAME_GAP;
        }

        // Top right: right aligned, dropped wholesale if it would run
        // into whatever sits on the left.
        let right_items: Vec<FrameButton> = buttons
            .iter()
            .filter(|b| b.slot == FrameSlot::TopRight)
            .cloned()
            .collect();
        let right_total: u16 = right_items.iter().map(|b| b.width()).sum::<u16>()
            + FRAME_GAP * (right_items.len().saturating_sub(1) as u16);
        if right_total > 0 && u32::from(right_total) <= u32::from(right - left + 1) {
            let start = right + 1 - right_total;
            if start > x {
                let mut cx = start;
                for b in &right_items {
                    out.buttons
                        .push((b.clone(), Rect::new(cx, top, b.width(), 1)));
                    cx += b.width() + FRAME_GAP;
                }
            }
        }

        // Bottom left: the action row, btop's terminate/kill/signals line.
        let mut cx = left;
        for b in buttons.iter().filter(|b| b.slot == FrameSlot::BottomLeft) {
            if !fits(cx, b.width()) {
                break;
            }
            out.buttons
                .push((b.clone(), Rect::new(cx, bottom, b.width(), 1)));
            cx += b.width() + FRAME_GAP;
        }

        out
    }

    /// Whether a button's word is drawn bold: btop marks a toggle that
    /// is currently on this way (`Fx::b` around `pause` while
    /// `pause_proc_list`, around `tree` while `proc_tree`, ...).
    fn frame_button_active(&self, id: ZoneId, button: &FrameButton) -> bool {
        match (id, button.key) {
            (ZoneId::Results, 'f') => self.zones.filter_mode || !self.zones.filter_input.is_empty(),
            (ZoneId::Torrent, 'p') => self.torrent_paused,
            _ => false,
        }
    }

    /// Draw `id`'s frame legend -- the buttons and the info text, on top
    /// of the border the panel's block has just drawn.
    fn render_frame(&self, frame: &mut Frame, id: ZoneId, area: Rect, config: &Config) {
        let layout = self.frame_layout(id, area, config);
        if layout.info.width > 0 {
            let info = Span::styled(
                layout.info_text,
                Style::default().fg(self.theme.title.to_color()),
            );
            frame.render_widget(Paragraph::new(Line::from(info)), layout.info);
        }
        for (button, rect) in &layout.buttons {
            let spans = super::zones::button_spans(
                &self.theme,
                button,
                self.frame_button_active(id, button),
            );
            frame.render_widget(Paragraph::new(Line::from(spans)), *rect);
        }
    }

    /// Perform a frame button's effect. The ones `ui::App` owns -- the
    /// filter prompt and the category arrows -- happen right here; the
    /// rest come back as a [`UiAction`] for the orchestrator, which owns
    /// the async work and the results list.
    ///
    /// `col` and `rect` are where the click landed: the category button's
    /// left and right arrow cells are separate targets (previous / next
    /// category), and the name between them is not a target at all.
    fn activate_frame_button(
        &mut self,
        id: ZoneId,
        button: &FrameButton,
        col: u16,
        rect: &Rect,
    ) -> Option<UiAction> {
        if id == ZoneId::Results && button.is_category() {
            if col == rect.x {
                self.cycle_group(false);
            } else if col == rect.x + rect.width.saturating_sub(1) {
                self.cycle_group(true);
            }
            return None;
        }
        match (id, button.key) {
            (ZoneId::Results, 'f') => {
                self.zones.filter_mode = true;
                None
            }
            (ZoneId::Results, 'g') => {
                self.cycle_group(true);
                None
            }
            (ZoneId::Results, '⏎') => Some(UiAction::Play),
            (ZoneId::Results, 'd') => Some(UiAction::Download),
            (ZoneId::Results, 'v') => Some(UiAction::Info),
            (ZoneId::Torrent, 'p') => Some(UiAction::TogglePause),
            (ZoneId::Torrent, 'd') => Some(UiAction::Remove),
            _ => None,
        }
    }

    /// Handle a left click anywhere in the main view: focuses whichever
    /// zone the click landed in (matching btop's click-to-focus), then
    /// tries the zone's frame legend (btop's buttons are click targets
    /// too), then the zone's own content -- a Results row, the category
    /// row, a Sources checkbox. Actions that need the orchestrator (an
    /// async TorrServer call, a search restart) are returned rather than
    /// performed here, since `ui::App` doesn't own that state.
    ///
    /// `config` is the live one: the Sources panel edits it, and the
    /// Results frame's info slot reads the selection it implies.
    pub fn click_at(&mut self, row: u16, col: u16, config: &mut Config) -> Option<UiAction> {
        let id = self.zone_at(row, col)?;
        let area = self.zones.get_area(id);
        self.zones.focused = id;

        // The legend sits on the border, outside every other hit target
        // of the panel, so it can be tested first without shadowing one.
        let layout = self.frame_layout(id, area, config);
        if let Some((button, rect)) = layout.button_at(col, row) {
            return self.activate_frame_button(id, &button, col, &rect);
        }

        match id {
            ZoneId::Results => {
                // -1 for the panel border: `table_row` is the 0-based line
                // inside the Results panel -- 0 = the table's own header
                // row, 1+ = data rows. This must stay in lockstep with
                // render_results_zone's Layout (table only now; the
                // category row moved onto the frame); 038c859 added the
                // tab row and subtracted its line here but left
                // `data_row`'s own -1, so every click used to select the
                // row *below* the one under the cursor and a click on the
                // header selected the first item -- the same class of
                // off-by-one the category row would have brought back if
                // only the draw side moved.
                let table_row = row.saturating_sub(area.y).saturating_sub(1);
                if table_row < 1 {
                    // Header row ("Seeds  Size ..."): not a data row.
                    return None;
                }
                let data_row = (table_row - 1) as usize;
                if let Some(&idx) = self.filtered_indices.get(data_row) {
                    self.selected = idx;
                }
            }
            ZoneId::Sources => {
                // The rows are the controls, so a click is the same as
                // moving the cursor there and pressing Enter.
                if let Some(row) = self.sources_row_at(row, col) {
                    self.sources_cursor = source_rows()
                        .iter()
                        .position(|r| *r == row)
                        .unwrap_or(self.sources_cursor);
                    self.toggle_source(config);
                }
            }
            ZoneId::Torrent => {
                // Pause and remove live on the frame now (btop's
                // terminate/kill row), handled by the legend test above.
            }
            ZoneId::Log | ZoneId::Extra => {}
        }
        None
    }

    /// Border+background styling for the four main zone panels,
    /// respecting the "Rounded corners" and "Theme background" Options
    /// toggles. Centralizes what used to be ~14 separate hand-rolled
    /// `Block::default()...` call sites, each of which would have needed
    /// this same two-setting check repeated -- previously these settings
    /// were persisted in Config but had no rendering effect anywhere.
    ///
    /// Modal popups use `modal_block` instead, not this: "Theme
    /// background" is about letting terminal transparency show through
    /// the regular panels, which is a different concern from whether a
    /// temporary popup dialog is legible on top of whatever's behind it.
    fn themed_block(&self, border_color: Color, config: &Config) -> Block<'static> {
        let border_color = self.resolve_color(border_color, config);
        let border_type = if config.rounded_corners && !config.false_tty {
            BorderType::Rounded
        } else {
            BorderType::Plain
        };
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(Style::default().fg(border_color));
        if config.theme_background {
            block = block.style(
                Style::default().bg(self.resolve_color(self.theme.main_bg.to_color(), config)),
            );
        }
        block
    }

    /// Border+background styling for modal popups (Settings, Login,
    /// HealthCheck). Respects "Theme background": when true, fills with
    /// the theme's `main_bg` (as before); when false, omits the `bg`
    /// style so the `Clear` rendered before the block (present in all
    /// three modals) hides the content underneath while the terminal's
    /// background color shows through. Still respects rounded corners and
    /// truecolor/false_tty degradation like every other themed block.
    pub(crate) fn modal_block(&self, border_color: Color, config: &Config) -> Block<'static> {
        let border_color = self.resolve_color(border_color, config);
        let border_type = if config.rounded_corners && !config.false_tty {
            BorderType::Rounded
        } else {
            BorderType::Plain
        };
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(Style::default().fg(border_color));
        if config.theme_background {
            block = block.style(
                Style::default().bg(self.resolve_color(self.theme.main_bg.to_color(), config)),
            );
        }
        block
    }

    /// Degrade an RGB color per the "Truecolor"/"False tty" toggles; see
    /// `theme::degrade_color`. Named/basic colors pass through untouched.
    fn resolve_color(&self, color: Color, config: &Config) -> Color {
        if config.false_tty {
            super::theme::degrade_color(color, false)
        } else if !config.truecolor {
            super::theme::degrade_color(color, true)
        } else {
            color
        }
    }

    pub fn enter_input_mode(&mut self) {
        self.input_mode = true;
    }

    pub fn exit_input_mode(&mut self) {
        self.input_mode = false;
    }

    pub fn type_char(&mut self, c: char) {
        if self.input_mode {
            self.search_input.push(c);
        }
    }

    pub fn backspace(&mut self) {
        if self.input_mode {
            self.search_input.pop();
        }
    }

    pub fn clear_input(&mut self) {
        self.search_input.clear();
    }

    pub fn delete_word(&mut self) {
        let words: Vec<&str> = self.search_input.split_whitespace().collect();
        if let Some(last) = words.last() {
            let cut_pos = self.search_input.len() - last.len();
            self.search_input.truncate(cut_pos);
        }
    }

    pub fn navigate_down(&mut self) -> bool {
        if !self.results.is_empty() && !self.input_mode && self.modal == Modal::None {
            let filtered_len = self.filtered_indices.len();
            if filtered_len == 0 {
                return false;
            }
            let local_idx = self
                .filtered_indices
                .iter()
                .position(|&i| i == self.selected)
                .unwrap_or(0);
            if local_idx < filtered_len - 1 {
                self.selected = self.filtered_indices[local_idx + 1];
                true
            } else {
                !self.all_loaded && self.state == AppState::Idle
            }
        } else {
            false
        }
    }

    pub fn needs_more(&self) -> bool {
        self.search_query.is_some()
            && !self.all_loaded
            && self.state == AppState::Idle
            && self.selected >= self.results.len().saturating_sub(3)
            && !self.results.is_empty()
    }

    pub fn navigate_up(&mut self) -> bool {
        if !self.input_mode && self.modal == Modal::None {
            let local_idx = self
                .filtered_indices
                .iter()
                .position(|&i| i == self.selected)
                .unwrap_or(0);
            if local_idx > 0 {
                self.selected = self.filtered_indices[local_idx - 1];
            }
            true
        } else {
            false
        }
    }

    pub fn navigate_first(&mut self) {
        if let Some(&first) = self.filtered_indices.first() {
            self.selected = first;
        }
    }

    pub fn navigate_last(&mut self) {
        if let Some(&last) = self.filtered_indices.last() {
            self.selected = last;
        }
    }

    pub fn quit(&mut self) {
        self.running = false;
    }

    pub fn submit_search(&mut self) -> Option<String> {
        if self.input_mode {
            let query = self.search_input.clone();
            self.input_mode = false;
            if !query.is_empty() {
                Some(query)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn submit_selection(&self) -> Option<usize> {
        if !self.input_mode && self.modal == Modal::None && !self.filtered_indices.is_empty() {
            Some(self.selected)
        } else {
            None
        }
    }

    /// Which rows the Results panel shows: the selected category first,
    /// then the `F` text filter on top of it.
    ///
    /// The category half is B6's *instant* side: switching the row
    /// re-derives this from the rows already on screen, so a selected
    /// category never sits above a table still showing every group.
    /// Rows a source could not attribute (`item.group = None`) belong to
    /// the "all" view only -- hiding them here is what makes that
    /// ROADMAP rule mean something instead of being a comment.
    pub fn update_filter(&mut self) {
        let filter = self.zones.filter_input.clone();
        let lower = filter.to_lowercase();
        // The filter matches any field a result carries, not just the
        // title: a size ("1.4 GB"), a source ("rutracker"), a word from
        // the title, or the category all answer to the same prompt --
        // the user should not have to know which column a term lives
        // in. The category is matched by name, so "movies" finds the
        // Movies view's rows.
        self.filtered_indices = self
            .results
            .iter()
            .enumerate()
            .filter(|(_, item)| match self.active_group {
                None => true,
                Some(group) => item.group == Some(group),
            })
            .filter(|(_, item)| {
                if filter.is_empty() {
                    return true;
                }
                let haystack = format!(
                    "{} {} {} {}",
                    item.title,
                    item.size,
                    item.source,
                    item.group.map_or("", Group::label),
                );
                haystack.to_lowercase().contains(&lower)
            })
            .map(|(i, _)| i)
            .collect();
        if !self.filtered_indices.is_empty() && !self.filtered_indices.contains(&self.selected) {
            self.selected = self.filtered_indices[0];
        }
    }

    /// Draw the main view. `config` rides along because the zones read
    /// it: the Sources panel's checkboxes and the Results frame's "what
    /// the search is asking" slot both come from `enabled_sources`.
    pub fn render(&mut self, frame: &mut Frame, config: &Config) {
        let area = frame.area();

        if self.show_menu {
            self.render_menu_view(frame, area, config);
            return;
        }

        self.render_main_view(frame, area, config);
    }

    fn render_menu_view(&mut self, frame: &mut Frame, area: Rect, config: &Config) {
        self.zones.update_areas(area);
        self.render_search_bar(frame, area, config);
        for zone_id in ZoneId::all() {
            let zone_area = self.zones.get_area(*zone_id);
            if zone_area.width == 0 || zone_area.height == 0 {
                continue;
            }
            match zone_id {
                ZoneId::Results => self.render_results_zone(frame, zone_area, *zone_id, config),
                ZoneId::Torrent => self.render_torrent_zone(frame, zone_area, *zone_id, config),
                ZoneId::Log => self.render_log_zone(frame, zone_area, *zone_id, config),
                ZoneId::Extra => self.render_extra_zone(frame, zone_area, *zone_id, config),
                ZoneId::Sources => self.render_sources_zone(frame, zone_area, *zone_id, config),
            }
        }
        super::menu::render_menu(frame, area, &self.menu, &self.theme);
    }

    fn render_main_view(&mut self, frame: &mut Frame, area: Rect, config: &Config) {
        self.zones.update_areas(area);

        if self.detail_log_mode {
            self.render_full_log(frame, area, config);
        } else {
            self.render_search_bar(frame, area, config);

            for zone_id in ZoneId::all() {
                let zone_area = self.zones.get_area(*zone_id);
                if zone_area.width == 0 || zone_area.height == 0 {
                    continue;
                }

                match zone_id {
                    ZoneId::Results => self.render_results_zone(frame, zone_area, *zone_id, config),
                    ZoneId::Torrent => self.render_torrent_zone(frame, zone_area, *zone_id, config),
                    ZoneId::Log => self.render_log_zone(frame, zone_area, *zone_id, config),
                    ZoneId::Extra => self.render_extra_zone(frame, zone_area, *zone_id, config),
                    ZoneId::Sources => self.render_sources_zone(frame, zone_area, *zone_id, config),
                }
            }
        }

        if self.modal != Modal::None {
            self.render_modal(frame, area, config);
        }
    }

    /// Whether `row` is inside the search input's box: the top
    /// [`SEARCH_BAR_HEIGHT`] rows of the frame, which `render_search_bar`
    /// is handed straight from `render` and `update_areas` leaves to the
    /// input instead of to any zone. The box is full-width, so the row
    /// alone decides.
    ///
    /// Clicking it starts editing -- the job the clickable header hints
    /// (`"s: search | ..."`) used to do before П.3 deleted them; the box
    /// itself is the natural target now that nothing else on that line
    /// is interactive. It is not a target while something paints over
    /// it: fullscreen stretches a zone across the whole frame and the
    /// detail log takes it too.
    pub fn search_box_at(&self, row: u16) -> bool {
        let covered = self.zones.fullscreen.is_some() || self.detail_log_mode;
        !covered && row < SEARCH_BAR_HEIGHT
    }

    fn render_search_bar(&self, frame: &mut Frame, area: Rect, config: &Config) {
        let bar_area = Rect::new(area.x, area.y, area.width, SEARCH_BAR_HEIGHT);

        // A label, not a keybind cheat-sheet: where the keys live is
        // the help page (`?`) and the frame legends now, and what this
        // box needs to say is what it is holding. The only thing that
        // changes is an active filter; the mode is the border colour
        // (yellow while typing into the query, cyan while the filter is
        // live) -- the same "colour says state, text says content"
        // split btop's boxes use.
        let filter_on = self.zones.filter_mode || !self.zones.filter_input.is_empty();
        // While `f` is open the text lives in the box, so the border
        // names the mode only -- repeating it there was the third copy
        // (border, body, frame legend). A filter in effect but not
        // being edited still announces itself: the box then holds the
        // query, and nothing else says a filter is on.
        let title = match (self.input_mode, self.zones.filter_mode, filter_on) {
            (false, true, _) => "filter".to_string(),
            (false, false, true) => format!("filter: {}", self.zones.filter_input),
            _ => "search".to_string(),
        };

        // The box holds whichever string is being edited: the query in
        // search mode, the filter while `f` is open. Before, the filter
        // only ever reached the border title, so the box kept showing
        // the stale query while the text grew somewhere else.
        let editing: &str = if self.zones.filter_mode {
            self.zones.filter_input.as_str()
        } else {
            self.search_input.as_str()
        };

        let input_border = self
            .themed_block(
                if self.input_mode {
                    Color::Yellow
                } else if self.zones.filter_mode {
                    Color::Cyan
                } else {
                    self.theme.div_line.to_color()
                },
                config,
            )
            .title(title);

        let inner = input_border.inner(bar_area);
        let input = Paragraph::new(editing)
            .block(input_border)
            .style(Style::default().fg(Color::White));

        frame.render_widget(input, bar_area);

        // A text field without a caret is a text field you type into
        // blind. The terminal hides it again on the next frame that
        // does not ask for one, so an idle box stays clean.
        if (self.input_mode || self.zones.filter_mode)
            && self.modal == Modal::None
            && !self.show_menu
        {
            let col = editing.chars().count() as u16;
            frame.set_cursor_position((inner.x + col.min(inner.width.saturating_sub(1)), inner.y));
        }
    }

    /// Whether the search's answer was "the network said no" rather
    /// than "nobody has it": every source that was dispatched ended in
    /// an error or a deadline. `Ok(0)` is an answer, not a failure.
    fn all_sources_failed(&self) -> bool {
        !self.source_status.is_empty()
            && self
                .source_status
                .values()
                .all(|s| matches!(s, SourceStatus::Error(_) | SourceStatus::Timeout))
    }

    /// What the Results panel says in place of an empty table.
    ///
    /// A blank table is five different situations wearing the same face
    /// -- nothing asked for yet, a search still running, a query that
    /// came back empty, every source down, and a filter that hid every
    /// row -- and the log was the only place that told them apart. The
    /// panel answers its own "why is this blank?".
    fn results_placeholder(&self) -> String {
        if self.state == AppState::Searching {
            return "Searching...".to_string();
        }
        if self.results.is_empty() && self.all_sources_failed() {
            return "Every source failed -- see the Sources panel".to_string();
        }
        if !self.zones.filter_input.is_empty() && !self.results.is_empty() {
            return format!(
                "Filter '{}' matches none of the {} rows",
                self.zones.filter_input,
                self.results.len()
            );
        }
        match &self.search_query {
            None => "Nothing searched yet -- press `s` to search".to_string(),
            Some(q) if q.is_empty() => "No fresh rows in this category".to_string(),
            Some(q) => format!("No results for '{q}'"),
        }
    }

    fn render_results_zone(&self, frame: &mut Frame, area: Rect, id: ZoneId, config: &Config) {
        let border_color = super::zones::zone_border_color(id, self.zones.focused, &self.theme);
        let block = self
            .themed_block(border_color, config)
            .title(super::zones::zone_title(id, &self.theme));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // One row inside the border: the table. The category row that
        // used to sit above it moved onto the frame (П.4 follow-up) --
        // the current category is read off the `◀ name ▶` button next to
        // `group`, so a row here would have been a second copy of it.
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0)]) // results table
            .split(inner);

        // --- results table ---------------------------------------------------
        // `Src` sits between the metadata and the title: on the `all`
        // tab a single page mixes trackers, and the row is the only
        // place that says who returned it.
        let header = Row::new(vec![
            Cell::from("Seeds"),
            Cell::from("Size"),
            Cell::from("Date"),
            Cell::from("Src"),
            Cell::from("Title"),
        ])
        .style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

        // Muted, but not `inactive_fg`: the tab bar gets away with that
        // one because a tab is also spelled out in the title. Here the
        // badge is the only thing saying who returned the row, and on
        // the `all` tab that is the point of the column -- so it takes
        // the informational mid-bright `graph_text` instead (≈6.7:1 on
        // `main_bg`, versus ≈2.3:1 for `inactive_fg`), which the cursor
        // row then writes in the theme's `selected_fg`.
        let badge_style = Style::default().fg(self.theme.graph_text.to_color());
        let rows: Vec<Row> = self
            .filtered_indices
            .iter()
            .filter_map(|&idx| self.results.get(idx))
            .map(|item| {
                Row::new(vec![
                    Cell::from(item.seeds.as_str()),
                    Cell::from(item.size.as_str()),
                    Cell::from(item.date.as_str()),
                    Cell::from(source_badge(item)).style(badge_style),
                    Cell::from(item.title.as_str()),
                ])
            })
            .collect();

        // Nothing to tabulate: say why instead of drawing the header
        // over an empty body, which read as a broken table. The block is
        // already on screen, so this only fills its inner area.
        if rows.is_empty() {
            let placeholder = Paragraph::new(self.results_placeholder())
                .style(Style::default().fg(self.theme.graph_text.to_color()))
                .wrap(Wrap { trim: true });
            frame.render_widget(placeholder, inner);
            self.render_frame(frame, id, area, config);
            return;
        }

        let table = Table::new(
            rows,
            [
                Constraint::Length(6),
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Length(SOURCE_BADGE_WIDTH),
                Constraint::Min(20),
            ],
        )
        .header(header)
        .row_highlight_style(self.theme.selection_style());

        let mut state = TableState::default();
        if let Some(local_pos) = self
            .filtered_indices
            .iter()
            .position(|&i| i == self.selected)
        {
            state.select(Some(local_pos));
        }
        frame.render_stateful_widget(table, chunks[0], &mut state);

        // The keybind legend moved onto the frame with П.5, so the panel
        // body ends at the table and every remaining line is data.
        self.render_frame(frame, id, area, config);
    }

    /// The Sources panel (П.4): the `all` master switch on top, then one
    /// row per registered source, `[x]`/`[ ]` showing whether the search
    /// asks it. The row under the cursor is reversed, the same way the
    /// selected result row is -- the cursor is the panel's only state, and
    /// it has to be visible the same way.
    fn render_sources_zone(&self, frame: &mut Frame, area: Rect, id: ZoneId, config: &Config) {
        let border_color = super::zones::zone_border_color(id, self.zones.focused, &self.theme);
        let block = self
            .themed_block(border_color, config)
            .title(super::zones::zone_title(id, &self.theme));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // The status reads as a column only if every id is padded to
        // the widest one, so an answer lands under the answer above it
        // instead of trailing each name at its own length.
        let roster = source_rows();
        let id_width = roster
            .iter()
            .map(|r| r.id().chars().count())
            .max()
            .unwrap_or(3);
        let rows: Vec<Line> = roster
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                let checked = row.is_checked(config);
                let mark = if checked { "x" } else { " " };
                let mut style = if !row.is_implemented() {
                    Style::default().fg(self.theme.inactive_fg.to_color())
                } else {
                    Style::default().fg(self.theme.main_fg.to_color())
                };
                let cursor = index == self.sources_cursor;
                if cursor {
                    style = self.theme.selection_style();
                }
                let mut spans = vec![Span::styled(
                    format!("[{}] {:<width$}", mark, row.id(), width = id_width),
                    style,
                )];
                if let SourceRow::One(source_id) = row {
                    if let Some(status) = self.source_status.get(source_id) {
                        let status_style = if cursor {
                            self.theme.selection_style()
                        } else {
                            source_status_style(status, &self.theme)
                        };
                        spans.push(Span::styled(
                            format!(" {}", source_status_text(status)),
                            status_style,
                        ));
                    }
                }
                // A planned source is listed -- so the next one is
                // visible where it will land -- but says so instead of
                // pretending it can be switched on.
                if !row.is_implemented() {
                    spans.push(Span::styled(" (planned)", style));
                }
                Line::from(spans)
            })
            .collect();

        // The panel can be turned off (`5`) and the terminal can be too
        // short for the roster, so the window follows the cursor -- the
        // rule the detail modal's file list already uses. Without it the
        // rows past the end were simply not drawn while `j`/`k` kept
        // walking every one of them, and the cursor could stand on a row
        // nobody could see.
        let visible = inner.height as usize;
        let offset = self
            .sources_cursor
            .saturating_sub(visible.saturating_sub(1));
        let shown: Vec<Line> = rows.into_iter().skip(offset).take(visible).collect();
        frame.render_widget(Paragraph::new(shown), inner);

        self.render_frame(frame, id, area, config);
    }

    fn render_torrent_zone(&self, frame: &mut Frame, area: Rect, id: ZoneId, config: &Config) {
        let s = &self.torrent_status;

        let progress_pct = (s.progress * 100.0) as u32;
        let bar_width = (area.width as usize).saturating_sub(4).min(50);

        let dl_speed = format_bytes(s.download_speed);
        let ul_speed = format_bytes(s.upload_speed);
        let dl_total = format_bytes(s.downloaded);
        let total = format_bytes(s.total_size);

        let status_display = if self.torrent_paused && !s.hash.is_empty() {
            format!("{} (paused)", s.status)
        } else {
            s.status.clone()
        };

        let history: Vec<f64> = self.progress_history.iter().copied().collect();
        let sparkline =
            super::widgets::graph::render_sparkline(&history, bar_width, &config.graph_symbol);

        // TorrServer has not named the torrent yet: from Enter to the
        // hash arriving there is real work in flight (session, magnet,
        // add, upload) and no value to print, and an empty
        // `Hash:   Status:` read as "nothing is happening".
        let header = if s.hash.is_empty() && self.state == AppState::Streaming {
            Line::from(vec![
                Span::styled("Status: ", Style::default().fg(Color::Yellow)),
                Span::raw("Starting stream..."),
            ])
        } else {
            Line::from(vec![
                Span::styled("Hash: ", Style::default().fg(Color::Yellow)),
                Span::raw(&s.hash),
                Span::styled("  Status: ", Style::default().fg(Color::Yellow)),
                Span::raw(status_display),
            ])
        };

        let lines = vec![
            header,
            Line::from(vec![
                Span::styled("Progress: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!("{} {}%", sparkline, progress_pct),
                    Style::default().fg(if progress_pct >= 100 {
                        Color::Green
                    } else {
                        Color::Cyan
                    }),
                ),
            ]),
            Line::from(vec![
                Span::styled("DL: ", Style::default().fg(Color::Green)),
                Span::raw(&dl_speed),
                Span::raw("  "),
                Span::styled("UL: ", Style::default().fg(Color::Blue)),
                Span::raw(&ul_speed),
            ]),
            Line::from(vec![
                Span::styled("Downloaded: ", Style::default().fg(Color::Yellow)),
                Span::raw(&dl_total),
                Span::raw(" / "),
                Span::raw(&total),
                Span::styled("  Seeds: ", Style::default().fg(Color::Yellow)),
                Span::raw(s.seeds.to_string()),
                Span::styled("  Peers: ", Style::default().fg(Color::Yellow)),
                Span::raw(s.peers.to_string()),
            ]),
        ];

        let border_color = super::zones::zone_border_color(id, self.zones.focused, &self.theme);
        let block = self
            .themed_block(border_color, config)
            .title(super::zones::zone_title(id, &self.theme));
        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, area);

        // "p: pause/resume  d: remove" is gone from the body: those two
        // are frame buttons now, top-right and bottom-left.
        self.render_frame(frame, id, area, config);
    }

    fn render_log_zone(&self, frame: &mut Frame, area: Rect, id: ZoneId, config: &Config) {
        let visible = (area.height as usize).saturating_sub(2);
        let offset = self.log_scroll.saturating_sub(visible);

        let visible_logs: Vec<Line> = self
            .logs
            .iter()
            .skip(offset)
            .take(visible)
            .map(|l| Line::from(l.as_str()))
            .collect();

        let border_color = super::zones::zone_border_color(id, self.zones.focused, &self.theme);
        let log_panel = Paragraph::new(visible_logs).block(
            self.themed_block(border_color, config)
                .title(super::zones::zone_title(id, &self.theme)),
        );

        frame.render_widget(log_panel, area);
        // The "(n/m)" scroll position moved from the title onto the
        // frame, next to the `detail` button.
        self.render_frame(frame, id, area, config);
    }

    fn render_extra_zone(&self, frame: &mut Frame, area: Rect, id: ZoneId, config: &Config) {
        let border_color = super::zones::zone_border_color(id, self.zones.focused, &self.theme);
        let block = self
            .themed_block(border_color, config)
            .title(super::zones::zone_title(id, &self.theme));
        let paragraph = Paragraph::new("Zone 4 — TBD").block(block);
        frame.render_widget(paragraph, area);
        self.render_frame(frame, id, area, config);
    }

    fn render_full_log(&self, frame: &mut Frame, area: Rect, config: &Config) {
        let total = self.detail_logs.len();
        let visible = (area.height as usize).saturating_sub(2);
        let scroll = self.detail_log_scroll.saturating_sub(visible);

        let lines: Vec<Line> = self
            .detail_logs
            .iter()
            .skip(scroll)
            .take(visible)
            .map(|l| {
                if l.contains("ERROR") || l.contains("FAIL") || l.contains("error:") {
                    Line::from(Span::styled(l.as_str(), Style::default().fg(Color::Red)))
                } else if l.contains("OK") || l.contains("SUCCESS") || l.contains("logged in") {
                    Line::from(Span::styled(l.as_str(), Style::default().fg(Color::Green)))
                } else if l.contains("WARN") {
                    Line::from(Span::styled(l.as_str(), Style::default().fg(Color::Yellow)))
                } else {
                    Line::from(l.as_str())
                }
            })
            .collect();

        let title = format!(
            " Detailed Log ({}/{}) [L/Esc] close [j/k] scroll ",
            scroll + visible.min(total),
            total
        );

        let log_panel =
            Paragraph::new(lines).block(self.themed_block(Color::Cyan, config).title(title));

        frame.render_widget(log_panel, area);
    }

    fn render_modal(&mut self, frame: &mut Frame, area: Rect, config: &Config) {
        if self.modal == Modal::None {
            return;
        }

        if let Modal::Login(_) = self.modal {
            self.render_login_modal(frame, area, config);
        } else if matches!(self.modal, Modal::Settings(_)) {
            self.render_settings_modal(frame, area, config);
        } else if let Modal::HealthCheck(_) = self.modal {
            self.render_health_modal(frame, area, config);
        } else if matches!(self.modal, Modal::Help(_)) {
            // `&mut self`: the page publishes its own page count for
            // `help_key` while it draws.
            self.render_help_modal(frame, area, config);
        } else if matches!(self.modal, Modal::TorrentDetail(_)) {
            self.render_detail_modal(frame, area, config);
        }
    }

    /// The detail modal's keys (П.7): j/k move the cursor through the
    /// file list, Enter plays the row, `d` downloads it, Esc/q close.
    ///
    /// Returns the actions that belong to the orchestrator; everything
    /// the modal owns itself (the cursor, closing) happens right here.
    pub fn detail_key(&mut self, key: KeyEvent, vim_keys: bool) -> Option<DetailAction> {
        let Modal::TorrentDetail(ref mut state) = self.modal else {
            return None;
        };
        match key.code {
            KeyCode::Char('j') if vim_keys => {
                if !state.files.is_empty() {
                    state.cursor = (state.cursor + 1).min(state.files.len() - 1);
                }
                None
            }
            KeyCode::Char('k') if vim_keys => {
                state.cursor = state.cursor.saturating_sub(1);
                None
            }
            KeyCode::Down => {
                if !state.files.is_empty() {
                    state.cursor = (state.cursor + 1).min(state.files.len() - 1);
                }
                None
            }
            KeyCode::Up => {
                state.cursor = state.cursor.saturating_sub(1);
                None
            }
            KeyCode::Enter => Some(DetailAction::Play),
            KeyCode::Char('d') => Some(DetailAction::Download),
            KeyCode::Esc | KeyCode::Char('q') => {
                self.modal = Modal::None;
                None
            }
            _ => None,
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
