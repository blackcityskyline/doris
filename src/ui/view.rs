use super::layout::{FrameButton, FrameSlot, ZoneId, ZoneLayout, SEARCH_BAR_HEIGHT};
use super::menu::MenuState;
use super::theme::Theme;
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
use std::path::PathBuf;

/// Where the search is.
#[derive(PartialEq)]
pub enum AppState {
    Idle,
    Searching,
    Streaming,
}

/// What a frame-button click (or the equivalent key) needs the orchestrator to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    Play,
    Download,
    Info,
    TogglePause,
    Remove,
    /// A Sources checkbox was switched (the panel's click path edits the
    /// config itself, so it has to say so for the orchestrator to
    /// persist it).
    TrackersChanged,
    /// The Results frame's category arrows were clicked: the same re-ask
    /// `g` fires, which needs an async caller -- `click_at` has none.
    ReaskCategory,
}

#[derive(PartialEq, Clone, Debug)]
pub enum Modal {
    None,
    Login(LoginState),
    Settings(SettingsState),
    HealthCheck(Vec<String>),
    Help(HelpState),
    /// The selected row's details: the row itself, the file list its source is still fetching
    /// (or has fetched), and the cursor into that list.
    TorrentDetail(Box<TorrentDetailState>),
}

/// What a key in the detail modal asks the orchestrator for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailAction {
    Play,
    Download,
}

/// The detail modal's state.
#[derive(Clone, Debug, PartialEq)]
pub struct TorrentDetailState {
    /// The row the modal was opened from.
    pub item: TorrentItem,
    /// The files inside the torrent, once `Source::details` answered.
    pub files: Vec<FileEntry>,
    /// `true` while that answer is in flight: the modal says so rather
    /// than leaving a blank list that reads as "no files".
    pub pending: bool,
    /// Why the file list could not be read, when it could not.
    pub error: Option<String>,
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
    /// Which zone has taken over the whole frame (`L`/`T`/`R`).
    pub detail_view: Option<ZoneId>,
    pub detail_log_scroll: usize,
    pub state: AppState,
    /// What each source answered for the running search: pending, how many rows, an error or a
    /// deadline.
    pub source_status: HashMap<String, SourceStatus>,
    pub torrserver_url: String,
    /// Where the Login modal's Ctrl+S writes the credential store.
    pub credentials_path: PathBuf,
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
    pub active_torrent_hash: Option<String>,
    /// A `d` on the Torrent zone has been pressed once and the removal is waiting for a second
    /// one.
    pub remove_armed: bool,
    /// Where the pointer is, when the terminal reports motion (`tui.rs` turns mode 1003 on).
    pub hover: Option<(u16, u16)>,
    /// Client-side pause tracking.
    pub torrent_paused: bool,
    /// Which row of the Trackers panel the cursor sits on: 0 is the `all` master switch, 1..
    pub sources_cursor: usize,
    /// Which category the Results table is showing -- the tab row under the frame; `None` is
    /// the "all" tab.
    pub active_group: Option<Group>,
    /// The category row itself: "all", then every group at least one enabled, implemented
    /// source serves, in `GROUP_ORDER`.
    pub group_tabs: Vec<Option<Group>>,
    /// Set when the category is switched (`g`/`G` or a click), cleared by the next
    /// `start_search`: Enter means "re-search with the new selection" for this row too.
    pub group_changed: bool,
    /// Browse mode: the current search is an empty query asking browse-capable sources for
    /// their freshest rows.
    pub browsing: bool,
    /// Set whenever the user switches the active source tab (via `]` key or mouse click).
    pub source_changed: bool,
    /// Set by `settings_key` right before it returns a cycle-type SettingsAction
    /// (CycleTheme/CyclePreset/etc): +1 for Right/Enter, -1 for Left.
    pub last_cycle_direction: i8,
    /// Rolling progress history feeding the Torrent panel's sparkline
    /// Oldest first; capped in app.rs's
    /// TorrentListUpdate handler so a long session doesn't grow this
    /// unboundedly.
    pub progress_history: std::collections::VecDeque<f64>,
    /// Session copy of the runtime browser visibility, refreshed from `App::browser_visibility`
    /// every time the settings modal opens.
    pub settings_browser_hidden: bool,
    pub filtered_indices: Vec<usize>,
    /// The row the cursor stood on before a filter pushed it off the list, kept so that
    /// widening the filter can put it back instead of leaving the cursor parked on the first
    /// match.
    pub filter_anchor: Option<usize>,
    /// Set when a search starts *without* dropping the rows -- see [`App::begin_search`].
    pub pending_clear: bool,
}

/// Width of the `Src` column in the results table.
pub const SOURCE_BADGE_WIDTH: u16 = 10;

/// How many log lines the ring buffer holds before it starts dropping the oldest.
const LOG_CAPACITY: usize = 500;

/// How many lines PgUp/PgDn move the log scroll.
pub const LOG_PAGE_STEP: usize = 10;

/// The `Src` cell for one result: the source id, or `-` when it is missing.
pub fn source_badge(item: &TorrentItem) -> String {
    if item.source.is_empty() {
        "-".to_string()
    } else {
        item.source.clone()
    }
}

/// One row of the Trackers panel: the `all` switch, then the registry in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRow {
    /// "Ask every checked source" -- the default view, and the row that
    /// checks or unchecks the whole roster in one keypress.
    All,
    /// One registered source, by id.
    One(&'static str),
}

impl SourceRow {
    /// What the row says on screen.
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

    /// The checkbox state this row draws, read from the config the panel edits.
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

/// What the Results frame's info slot says the search is asking: `all` when every implemented
/// source is checked (the default view), `none` when nothing is, otherwise the checked ids in
/// registry order.
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
            detail_view: None,
            detail_log_scroll: 0,
            state: AppState::Idle,
            source_status: HashMap::new(),
            torrserver_url,
            credentials_path: crate::credentials::credentials_path(),
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
            remove_armed: false,
            hover: None,
            torrent_paused: false,
            // Which row of the Trackers panel the cursor sits on: 0 is
            // the `all` switch, 1.. the registry entries.
            sources_cursor: 0,
            active_group: None,
            // A fresh install's view: every implemented source is on by
            // default. `App::new` immediately re-derives it from the
            // config actually being loaded, so this is only what shows
            // for the frame before that happens -- and it has to be the
            // real first-run config, not the raw `Config::default()`,
            // which carries no source list at all.
            group_tabs: group_tabs(&crate::sources::source::first_run()),
            group_changed: false,
            browsing: false,
            source_changed: false,
            last_cycle_direction: 1,
            progress_history: std::collections::VecDeque::new(),
            settings_browser_hidden: false,
            filtered_indices: Vec::new(),
            filter_anchor: None,
            pending_clear: false,
        }
    }

    /// The start of a search for `query`, decided in one place: a *re-ask* of the query already
    /// on screen (a category switch, which `g` fires itself now) keeps the rows the user is
    /// looking at until the first answer of the new round lands -- blanking the table on every
    /// `g` would trade one wrong answer for a flicker.
    pub fn begin_search(&mut self, query: &str) {
        let reask = self.search_query.as_deref() == Some(query);
        self.state = AppState::Searching;
        self.search_query = Some(query.to_string());
        self.all_loaded = false;
        // Whatever a tab switch owed this point is now paid: the search
        // below runs against the selection as it stands, so Enter goes
        // back to meaning "play" instead of restarting (`group_changed`
        // and the older `source_changed` clear here for that reason).
        self.source_changed = false;
        self.group_changed = false;
        if reask && !self.results.is_empty() {
            self.pending_clear = true;
        } else {
            self.drop_results();
        }
    }

    /// The rows no longer answer for the selection above them: drop
    /// them, the cursor with them, and re-match what is left.
    pub fn drop_results(&mut self) {
        self.results.clear();
        self.selected = 0;
        self.filter_anchor = None;
        self.pending_clear = false;
        self.update_filter();
    }

    /// Spend a `pending_clear` the moment it is certain no answer is
    /// coming (nothing checked, or nothing to dispatch): holding rows a
    /// question nobody is answering left behind is the same lie with a
    /// delay.
    pub fn take_pending_clear(&mut self) {
        if self.pending_clear {
            self.drop_results();
        }
    }

    pub fn add_log(&mut self, msg: &str) {
        let ts = chrono::Local::now().format("%H:%M:%S").to_string();
        // Follow the reader, not the writer: a new line only drags the
        // panel down if it was already parked at the bottom. A reader
        // halfway up a history keeps the history they were reading.
        let following = self.log_scroll >= self.logs.len();
        self.logs.push_back(format!("[{}] {}", ts, msg));
        if self.logs.len() > LOG_CAPACITY {
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

    /// Open `id`'s detail view, or close it if it is the one already
    /// showing -- the key that opens a takeover is the key that ends it.
    pub fn toggle_detail_view(&mut self, id: ZoneId) {
        self.detail_view = if self.detail_view == Some(id) {
            None
        } else {
            Some(id)
        };
    }

    /// Move the Log panel's scroll position by `delta` lines, clamped to the lines there are.
    pub fn scroll_logs(&mut self, delta: isize) {
        let end = self.logs.len() as isize;
        self.log_scroll = (self.log_scroll as isize + delta).clamp(0, end) as usize;
    }

    /// The full Log view's scroll, same clamping, its own position.
    pub fn scroll_detail_log(&mut self, delta: i64) {
        let end = self.detail_logs.len() as i64;
        self.detail_log_scroll = (self.detail_log_scroll as i64 + delta).clamp(0, end) as usize;
    }

    /// Switch the category row to `group` -- the single path behind both `g`/`G` and a click on
    /// the row.
    pub fn set_group(&mut self, group: Option<Group>) {
        if self.active_group == group {
            return;
        }
        self.active_group = group;
        self.group_changed = true;
        self.update_filter();
    }

    /// Move the category row one tab, forward for `g` and back for `G` (wraps).
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

    /// [`App::new`] takes no config, so the caller that *does* have
    /// one applies the rows it implies; chaining keeps that from being
    /// an easy line to forget at construction.
    pub fn with_group_tabs(mut self, config: &Config) -> Self {
        self.set_group_tabs(config);
        self
    }

    /// Re-derive the category row from `config` and repair `active_group` when the tab it
    /// pointed at has just disappeared (a source switched off in the Trackers panel can take a
    /// group with it when no other enabled source serves it).
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

    /// Move the Trackers panel's cursor by `delta` rows, wrapping both ways
    /// -- btop wraps its lists too, so the panel never dead-ends.
    pub fn navigate_trackers(&mut self, delta: i64) {
        let len = source_rows().len() as i64;
        if len == 0 {
            return;
        }
        let next = (self.sources_cursor as i64 + delta).rem_euclid(len);
        self.sources_cursor = next as usize;
    }

    /// The row of the Trackers panel under `(row, col)`, given that zone's current area: the
    /// top border, then one row per entry of [`source_rows`], starting one column in.
    pub fn sources_row_at(&self, row: u16, _col: u16) -> Option<SourceRow> {
        let area = self.zones.get_area(ZoneId::Trackers);
        if area.width == 0 || area.height == 0 {
            return None;
        }
        let line = row.checked_sub(area.y)?;
        // -1 for the panel border: line 0 inside the box is the first row.
        let index = line.checked_sub(1)? as usize;
        source_row_at(index)
    }

    /// Switch the row under the cursor in `config`, then re-derive the category row (a source
    /// switched off can take a group with it) and tell Enter that the enabled set owes a
    /// search.
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

    /// Which zone (if any) contains screen position `(row, col)`, honoring fullscreen mode
    /// (only the fullscreened zone is hit-testable while active).
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

    /// The text a zone shows next to its frame buttons: the sources the search asks and the row
    /// counts for Results, the checked count for Sources, the scroll position for Log.
    pub fn frame_info(&self, id: ZoneId, area: Rect, config: &Config) -> String {
        match id {
            ZoneId::Results => {
                // The counter leads, zero-padded and directly after the
                // zone's name; which sources are checked is the Trackers
                // panel's answer, so it is not repeated here (printing
                // it twice is how the two disagree).
                let counts = format!(
                    "({:03}/{:03})",
                    self.filtered_indices.len(),
                    self.results.len()
                );
                if self.zones.filter_input.is_empty() {
                    counts
                } else {
                    format!("{} [F: {}]", counts, self.zones.filter_input)
                }
            }
            ZoneId::Trackers => {
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
            ZoneId::Torrent => String::new(),
        }
    }

    /// Screen rects for `id`'s frame buttons and info text.
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

        let mut buttons = super::layout::zone_buttons(id);
        out.info_text = self.frame_info(id, area, config);
        let info_width = out.info_text.chars().count() as u16;

        // The category button is built here rather than in the static
        // table because its label names the current category: btop's
        // `◀ name ▶` sort header, with the two arrows as mouse targets
        // (previous / next category). The `g`/`G` keys stay the
        // keyboard way in, exactly as they were when the category was a
        // row inside the panel.
        //
        // The name is centred in a slot as wide as the widest category
        // so the arrows stay in the same columns no matter which one is
        // showing -- `◀..TV..▶` and `◀Movies▶` line up, instead of the
        // right arrow sliding four columns to the right on the longer
        // name (extra padding lands on the right, as in btop's headers).
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
                label: format!("◀ {:^width$} ▶", name, width = width),
            });
        }

        // Top left: the title already claims `zone_title_width` columns
        // after the border, then the buttons, then the info text.
        let mut x = left + super::layout::zone_title_width(id);
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

    /// Perform a frame button's effect.
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
            } else {
                return None;
            }
            return Some(UiAction::ReaskCategory);
        }
        match (id, button.key) {
            (ZoneId::Results, 'f') => {
                self.zones.filter_mode = true;
                None
            }
            (ZoneId::Results, 'g') => {
                self.cycle_group(true);
                Some(UiAction::ReaskCategory)
            }
            (ZoneId::Results, '⏎') => Some(UiAction::Play),
            (ZoneId::Results, 'd') => Some(UiAction::Download),
            (ZoneId::Results, 'v') => Some(UiAction::Info),
            (ZoneId::Torrent, 'p') => Some(UiAction::TogglePause),
            (ZoneId::Torrent, 'd') => Some(UiAction::Remove),
            _ => None,
        }
    }

    /// Handle a left click anywhere in the main view: focuses whichever zone the click landed
    /// in (matching btop's click-to-focus), then tries the zone's frame legend (btop's buttons
    /// are click targets too), then the zone's own content -- a Results row, the category row,
    /// a Sources checkbox.
    pub fn click_at(&mut self, row: u16, col: u16, config: &mut Config) -> Option<UiAction> {
        let id = self.zone_at(row, col)?;
        let area = self.zones.get_area(id);
        self.zones.focused = id;

        // The legend sits on the border, outside every other hit target
        // of the panel, so it can be tested first without shadowing one.
        let layout = self.frame_layout(id, area, config);
        if let Some((button, rect)) = layout.button_at(col, row) {
            // A legend click is a legend click: it never arms the drag,
            // so `pause` on its own border cannot be a resize handle.
            return self.activate_frame_button(id, &button, col, &rect);
        }

        // On a border that separates two zones, this click arms the
        // drag (the release ends it, the move does the work). The focus
        // above still happened: a border click used to select the panel
        // and it still does, it just may also move the divider.
        if self.zones.resize_start(row, col) {
            return None;
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
                    // Header row ("Seeds Size..."): not a data row.
                    return None;
                }
                let data_row = (table_row - 1) as usize;
                if let Some(&idx) = self.filtered_indices.get(data_row) {
                    self.move_selection_to(idx);
                }
            }
            ZoneId::Trackers => {
                // The rows are the controls, so a click is the same as
                // moving the cursor there and pressing Enter -- except
                // that Enter's `!input_mode` gate applies here too: the
                // search box has no zone of its own, so a query can be
                // half-typed while the pointer is over the panel, and
                // that click belongs to the query, not to the checkbox.
                if let Some(row) = self.sources_row_at(row, col) {
                    self.sources_cursor = source_rows()
                        .iter()
                        .position(|r| *r == row)
                        .unwrap_or(self.sources_cursor);
                    if !self.input_mode {
                        self.toggle_source(config);
                        return Some(UiAction::TrackersChanged);
                    }
                }
            }
            ZoneId::Torrent => {
                // Pause and remove live on the frame now (btop's
                // terminate/kill row), handled by the legend test above.
            }
            ZoneId::Log => {}
        }
        None
    }

    /// One difference, and it is deliberate: a dialog keeps its border even with "Show boxes"
    /// off.
    pub(crate) fn modal_block(&self, border_color: Color, config: &Config) -> Block<'static> {
        self.themed_block_with_borders(border_color, config, Borders::ALL)
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
                self.move_selection_to(self.filtered_indices[local_idx + 1]);
                true
            } else {
                !self.all_loaded && self.state == AppState::Idle
            }
        } else {
            false
        }
    }

    /// True when the list is at (or within three rows of) its end and the server says there is
    /// another page.
    pub fn needs_more(&self) -> bool {
        self.search_query.is_some()
            && !self.all_loaded
            && self.state == AppState::Idle
            && self.selected >= self.results.len().saturating_sub(3)
    }

    pub fn navigate_up(&mut self) -> bool {
        if !self.input_mode && self.modal == Modal::None {
            let local_idx = self
                .filtered_indices
                .iter()
                .position(|&i| i == self.selected)
                .unwrap_or(0);
            if local_idx > 0 {
                self.move_selection_to(self.filtered_indices[local_idx - 1]);
            }
            true
        } else {
            false
        }
    }

    /// Move the selection a page at a time, for `PageUp`/`PageDown` in the Results panel.
    pub fn navigate_page(&mut self, page: isize) -> bool {
        if self.results.is_empty()
            || self.input_mode
            || self.modal != Modal::None
            || self.filtered_indices.is_empty()
        {
            return false;
        }
        let last = self.filtered_indices.len() - 1;
        let here = self
            .filtered_indices
            .iter()
            .position(|&i| i == self.selected)
            .unwrap_or(0);
        let target = (here as isize + page).clamp(0, last as isize) as usize;
        if target != here {
            self.move_selection_to(self.filtered_indices[target]);
        }
        true
    }

    /// Note where the pointer is.
    pub fn set_hover(&mut self, row: u16, col: u16) -> bool {
        match self.hover {
            Some((r, c)) if r == row && c == col => false,
            _ => {
                self.hover = Some((row, col));
                true
            }
        }
    }

    /// The pointer is over a frame button, given where that button is drawn.
    pub fn hovers(&self, rect: Rect) -> bool {
        self.hover.is_some_and(|(row, col)| {
            (rect.y..rect.y + rect.height).contains(&row)
                && (rect.x..rect.x + rect.width).contains(&col)
        })
    }

    /// `d` on the Torrent zone.
    pub fn confirm_remove(&mut self) -> bool {
        if self.remove_armed {
            self.remove_armed = false;
            true
        } else {
            self.remove_armed = true;
            false
        }
    }

    /// Drop an armed removal, so the next `d` asks again instead of
    /// removing.
    pub fn disarm_remove(&mut self) {
        self.remove_armed = false;
    }

    /// The question line the Torrent zone shows while a removal is armed.
    pub fn remove_prompt(&self) -> Option<String> {
        self.remove_armed
            .then(|| "Remove this torrent? d again to confirm, any other key to cancel".to_string())
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

    /// Which rows the Results panel shows: the selected category first, then the `F` text
    /// filter on top of it.
    pub fn update_filter(&mut self) {
        let filter = crate::filter::Filter::parse(&self.zones.filter_input);
        // The filter matches any field a result carries, not just the
        // title -- a size, a source, a category or a word from the
        // title all answer to the same prompt -- and `field:value`
        // narrows it further (src/filter.rs holds the syntax).
        let visible: Vec<usize> = self
            .results
            .iter()
            .enumerate()
            .filter(|(_, item)| match self.active_group {
                None => true,
                Some(group) => item.group == Some(group),
            })
            .filter(|(_, item)| filter.matches(item))
            .map(|(i, _)| i)
            .collect();

        // The row the cursor was pushed off is back on screen: stand on
        // it again. Checked before the jump below so that a filter that
        // widens back over both the anchor and wherever the cursor has
        // since landed prefers the anchor -- that is the row the user
        // was reading when they started typing.
        let hidden = !visible.is_empty() && !visible.contains(&self.selected);
        match (self.filter_anchor, hidden) {
            // The row the cursor came from is visible again: stand on
            // it. This wins over the jump below, which is the point --
            // the anchor is the row the user was reading.
            (Some(anchor), _) if visible.contains(&anchor) => {
                self.selected = anchor;
                self.filter_anchor = None;
            }
            // Already displaced, and pushed off again by a narrower
            // filter: keep the original anchor, only move the cursor.
            (Some(_), true) => self.selected = visible[0],
            // First displacement: remember the row being left behind.
            (None, true) => {
                self.filter_anchor = Some(self.selected);
                self.selected = visible[0];
            }
            _ => {}
        }
        self.filtered_indices = visible;
    }

    /// Move the cursor by the user's own hand (keys, click).
    fn move_selection_to(&mut self, idx: usize) {
        self.filter_anchor = None;
        self.selected = idx;
    }

    /// Whether `row` is inside the search input's box: the top [`SEARCH_BAR_HEIGHT`] rows of
    /// the frame, which `render_search_bar` is handed straight from `render` and `update_areas`
    /// leaves to the input instead of to any zone.
    pub fn search_box_at(&self, row: u16) -> bool {
        let covered = self.zones.fullscreen.is_some() || self.detail_view.is_some();
        !covered && row < SEARCH_BAR_HEIGHT
    }

    /// Whether the search's answer was "the network said no" rather than "nobody has it": every
    /// source that was dispatched ended in an error or a deadline.
    fn all_sources_failed(&self) -> bool {
        !self.source_status.is_empty()
            && self
                .source_status
                .values()
                .all(|s| matches!(s, SourceStatus::Error(_) | SourceStatus::Timeout))
    }

    /// What the Results panel says in place of an empty table.
    pub(super) fn results_placeholder(&self) -> String {
        if self.state == AppState::Searching {
            return "Searching...".to_string();
        }
        if self.results.is_empty() && self.all_sources_failed() {
            return "Every source failed -- see the Trackers panel".to_string();
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

    /// The detail modal's keys: j/k move the cursor through the file list, Enter plays the row,
    /// `d` downloads it, Esc/q close.
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

#[cfg(test)]
mod colour_tests {
    // Both moved to `render.rs` with the log panel that draws them; the
    // tests came along because they are the reason those two functions
    // read a colour rather than return one.
    use crate::sources::orchestrator::SourceStatus;
    use crate::ui::draw::{detail_log_style, source_status_style};
    use crate::ui::theme::Theme;
    use ratatui::style::Color;

    /// Severity decides the accent, and the accent is the theme's:
    /// these lines were hardcoded red/green/yellow before the tokens,
    /// so a theme could recolour the app and leave the log behind.
    #[test]
    fn test_detail_log_lines_take_their_severity_from_the_theme() {
        let theme = Theme::dark();
        assert_eq!(
            detail_log_style("ERROR: fetch failed", &theme).fg,
            Some(theme.error_color())
        );
        assert_eq!(
            detail_log_style("logged in OK", &theme).fg,
            Some(theme.secondary_color())
        );
        assert_eq!(
            detail_log_style("WARN: slow source", &theme).fg,
            Some(theme.primary_color())
        );
        assert_eq!(
            detail_log_style("searching rutor", &theme).fg,
            Some(theme.main_fg.to_color())
        );

        let mut accented = Theme::dark();
        accented.primary = Some(crate::ui::theme::ColorDef::new(1, 2, 3));
        assert_eq!(
            detail_log_style("WARN: slow source", &accented).fg,
            Some(Color::Rgb(1, 2, 3)),
            "a theme that spells the token out wins over the fallback"
        );
    }

    /// A refusal reads as the error accent wherever a source reports
    /// one -- the Trackers panel marks the same fact in words.
    #[test]
    fn test_a_refusing_source_reads_as_the_error_accent() {
        let theme = Theme::dark();
        assert_eq!(
            source_status_style(&SourceStatus::Error("403".into()), &theme).fg,
            Some(theme.error_color())
        );
        assert_eq!(
            source_status_style(&SourceStatus::Timeout, &theme).fg,
            Some(theme.error_color())
        );
        assert_eq!(
            source_status_style(&SourceStatus::Pending, &theme).fg,
            Some(theme.inactive_fg.to_color())
        );
        assert_eq!(
            source_status_style(&SourceStatus::Ok(3), &theme).fg,
            Some(theme.graph_text.to_color())
        );
    }
}
