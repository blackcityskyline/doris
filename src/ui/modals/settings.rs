//! The settings modal (ROADMAP Phase 10 split, item 1 of 3 -- and the
//! largest): the typed descriptor table the Options modal renders, its
//! pagination and key handling, and the per-category item builders
//! (general/streaming/download plus the derived sources list). The
//! `App` struct and the `Modal` enum stay in `ui/app.rs`; this file is
//! `impl App` blocks for just this modal's piece, the same pattern
//! `ui/modals/login.rs` and `health.rs` use.

use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui::Frame;

use crate::config::Config;
use crate::sources::source::{Group, GROUP_ORDER, KNOWN_SOURCES};
use crate::ui::app::{centered_rect, App, Modal};

/// Column where the settings modal draws its vertical divider between
/// the option name and its value. Capped at `bw - 3` so a narrow modal
/// still has room for the value.
const SETTINGS_DIVIDER_COL: usize = 30;

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsState {
    pub selected_category: usize,
    pub selected: usize,
    pub page: usize,
    /// Items shown per page, computed from the real terminal size the last
    /// time this modal was rendered. `settings_key`'s pagination reads this
    /// instead of guessing, so paging can never desync from what's on
    /// screen (see ROADMAP.md bug B2). Starts at 1 (never 0, which would
    /// divide-by-zero in pagination math) until the first render sets it.
    pub visible_items: usize,
    pub categories: Vec<SettingsCategory>,
    /// `(index, total)` of the *theme* the settings show, not of the row
    /// that shows them: the one number on this modal that answers "which
    /// one am I on" for a value with an order of its own. Measured in
    /// `open_settings`, the same place the theme is read from disk, so
    /// the renderer never has to load the theme files to draw a label.
    pub theme_pos: Option<(usize, usize)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsCategory {
    pub name: String,
    pub items: Vec<SettingsItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsItem {
    pub label: String,
    pub value: String,
    pub description: Vec<String>,
    pub action: SettingsAction,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingsAction {
    ToggleBrowserVisibility,
    CyclePrioritizeBrowser,
    ToggleMode,
    ToggleCloseBrowserOnExit,
    ToggleSaveCookies,
    ToggleSaveCredentials,
    ToggleEnableTorrserver,
    EditCredentials,
    CheckTorrserverStatus,
    ToggleDownloadEnabled,
    CycleDownloadDirMode,
    ToggleCloseTorrentCoreOnExit,
    RunHealthCheck,
    OpenLog,
    CycleTheme,
    ToggleThemeBackground,
    ToggleTruecolor,
    ToggleFalseTty,
    ToggleVimKeys,
    ToggleMouse,
    ToggleDisablePresets,
    CyclePreset,
    ToggleShowBoxes,
    SetUpdateMs,
    ToggleRoundedCorners,
    ToggleTerminalSync,
    CycleGraphSymbol,
    ToggleSaveOnExit,
    Close,
}

fn bool_str(b: bool) -> String {
    if b {
        "True".into()
    } else {
        "False".into()
    }
}

/// The category row's tabs: "all", then -- in `GROUP_ORDER` -- every
/// group that at least one enabled, implemented source serves.
///
/// Availability rather than a fixed four (the layout B6's question was
/// asked in): a category no enabled source could answer would be a tab
/// that can only show an empty table with no explanation, which is the
/// trap the Trackers panel's rows avoid for unchecked sources.
pub fn group_tabs(config: &Config) -> Vec<Option<Group>> {
    let available = |group: Group| {
        KNOWN_SOURCES
            .iter()
            .filter(|info| info.implemented)
            .filter(|info| config.enabled_sources.iter().any(|e| e == info.id))
            .any(|info| info.groups.contains(&group))
    };
    let mut tabs = vec![None];
    tabs.extend(
        GROUP_ORDER
            .into_iter()
            .filter(|&group| available(group))
            .map(Some),
    );
    tabs
}

/// The Options "streaming" category's items, built from the config the
/// same way [`group_tabs`] builds the category row -- one place, so a
/// test can assert what the category offers without a rendered modal.
/// `browser_hidden` is the runtime UI state the
/// `mode_str` is the "Play mode" row's value, computed by the caller.
pub fn streaming_settings_items(
    config: &Config,
    browser_hidden: bool,
    mode_str: &str,
) -> Vec<SettingsItem> {
    let visibility_str = if browser_hidden {
        "Hidden".to_string()
    } else {
        "Visible".to_string()
    };
    let items: Vec<SettingsItem> = vec![
        SettingsItem {
            label: "Browser visible".into(),
            value: visibility_str,
            description: vec![
                "Show or hide the automated".into(),
                "browser window.".into(),
                "".into(),
                "\"Hidden\" (default) runs it in".into(),
                "the background.".into(),
                "\"Visible\" shows the real".into(),
                "browser window.".into(),
                "".into(),
                "Applies the next time a".into(),
                "browser is launched.".into(),
            ],
            action: SettingsAction::ToggleBrowserVisibility,
        },
        SettingsItem {
            label: "Prioritize browser".into(),
            value: config
                .browser_priority
                .first()
                .cloned()
                .unwrap_or_else(|| "auto".into()),
            description: vec![
                "Which installed browser to".into(),
                "try first.".into(),
                "".into(),
                "Cycles chrome / chromium /".into(),
                "brave / helium. Whichever is".into(),
                "actually installed wins; this".into(),
                "only changes probe order.".into(),
            ],
            action: SettingsAction::CyclePrioritizeBrowser,
        },
        SettingsItem {
            label: "Play mode".into(),
            value: mode_str.to_string(),
            description: vec![
                "Set playback mode.".into(),
                "".into(),
                "\"Streaming\" uses TorrServer,".into(),
                "\"Download\" saves .torrent files.".into(),
            ],
            action: SettingsAction::ToggleMode,
        },
        SettingsItem {
            label: "Close browser on exit".into(),
            value: bool_str(config.close_browser_on_exit),
            description: vec![
                "Kill the automated browser".into(),
                "when Doris exits.".into(),
                "".into(),
                "Set to False to leave it".into(),
                "running after Doris closes.".into(),
            ],
            action: SettingsAction::ToggleCloseBrowserOnExit,
        },
        SettingsItem {
            label: "Save cookies".into(),
            value: bool_str(config.save_cookies),
            description: vec![
                "Persist session cookies to".into(),
                "the cookie file so logins".into(),
                "survive a restart.".into(),
            ],
            action: SettingsAction::ToggleSaveCookies,
        },
        SettingsItem {
            label: "Save credentials".into(),
            value: bool_str(config.save_credentials),
            description: vec![
                "Remember username/password".into(),
                "(encrypted) after a login.".into(),
            ],
            action: SettingsAction::ToggleSaveCredentials,
        },
        SettingsItem {
            label: "Edit credentials".into(),
            value: "press Enter".into(),
            description: vec![
                "Open the login panel to".into(),
                "view or change saved logins.".into(),
            ],
            action: SettingsAction::EditCredentials,
        },
        SettingsItem {
            label: "Enable TorrServer".into(),
            value: bool_str(config.enable_torrserver),
            description: vec![
                "Stream through TorrServer.".into(),
                "".into(),
                "Off means Doris never".into(),
                "reaches for it, and says".into(),
                "so instead of failing.".into(),
            ],
            action: SettingsAction::ToggleEnableTorrserver,
        },
        SettingsItem {
            label: "TorrServer".into(),
            value: "press Enter to check".into(),
            description: vec![
                "Check whether TorrServer is".into(),
                "reachable right now.".into(),
            ],
            action: SettingsAction::CheckTorrserverStatus,
        },
    ];
    items
}

fn center_str(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.to_string()
    } else {
        let pad = width - len;
        let left = pad / 2;
        let right = pad - left;
        format!("{}{}{}", " ".repeat(left), s, " ".repeat(right))
    }
}

impl App {
    /// Build the Settings modal from real, current state. Every `value`
    /// here is computed from `self`/`config`, never a hardcoded literal --
    /// see ROADMAP.md bug B5, where roughly half of these used to be
    /// decorative strings with no backing field at all.
    pub fn open_settings(&mut self, config: &Config, browser_hidden: bool) {
        self.settings_browser_hidden = browser_hidden;
        let mode_str = if self.stream_mode {
            "Streaming (TorrServer)".to_string()
        } else {
            "Download (.torrent file)".to_string()
        };
        let theme_name = self.theme.name.clone();
        // Value shown between the cycle arrows must be the theme's own
        // name (matching the reference: "<- noctalia ->"), not a bare
        // index/total -- that's genuinely useful information but belongs
        // in the *label* position indicator every settings item already
        // gets when selected ("Color theme 7/45"), not here. The row's
        // own "1/15" is the row's position in the category and answered
        // neither question, which is why the label carries this instead.
        let theme_str = theme_name.clone();
        let themes = crate::ui::theme::Theme::load_themes();
        let theme_pos = themes
            .iter()
            .position(|t| t.name == theme_name)
            .map(|i| (i + 1, themes.len()));

        let preset_str = config
            .presets
            .get(config.preset_index)
            .cloned()
            .unwrap_or_else(|| "none".to_string());
        let preset_display = format!(
            "{} ({}/{})",
            preset_str,
            config.preset_index + 1,
            config.presets.len().max(1),
        );

        // Retain the previously-selected position within the previously-
        // selected category (if any) so re-rendering after a toggle
        // doesn't silently reset scroll position back to the top item.
        let (prev_category, prev_selected, prev_page, prev_visible_items) =
            if let Modal::Settings(ref prev) = self.modal {
                (
                    prev.selected_category,
                    prev.selected,
                    prev.page,
                    prev.visible_items,
                )
            } else {
                (0, 0, 0, 1)
            };

        self.modal = Modal::Settings(SettingsState {
            selected_category: prev_category,
            selected: prev_selected,
            page: prev_page,
            visible_items: prev_visible_items.max(1),
            theme_pos,
            categories: vec![
                SettingsCategory {
                    name: "general".into(),
                    items: vec![
                        SettingsItem {
                            label: "Color theme".into(),
                            value: theme_str,
                            description: vec![
                                "Set color theme.".into(),
                                "".into(),
                                "Choose from all theme files in".into(),
                                "\"~/.config/doris/themes\".".into(),
                                "".into(),
                                "Use Left/Right to cycle.".into(),
                            ],
                            action: SettingsAction::CycleTheme,
                        },
                        SettingsItem {
                            label: "Theme background".into(),
                            value: bool_str(config.theme_background),
                            description: vec![
                                "If the theme set background".into(),
                                "should be shown.".into(),
                                "".into(),
                                "Set to False if you want".into(),
                                "terminal background".into(),
                                "transparency.".into(),
                            ],
                            action: SettingsAction::ToggleThemeBackground,
                        },
                        SettingsItem {
                            label: "Truecolor".into(),
                            value: bool_str(config.truecolor),
                            description: vec![
                                "Sets if 24-bit truecolor".into(),
                                "should be used.".into(),
                                "".into(),
                                "Will convert 24-bit colors to".into(),
                                "256 color if False.".into(),
                                "".into(),
                                "Set to False if your terminal".into(),
                                "doesn't have truecolor".into(),
                                "support.".into(),
                            ],
                            action: SettingsAction::ToggleTruecolor,
                        },
                        SettingsItem {
                            label: "False tty".into(),
                            value: bool_str(config.false_tty),
                            description: vec![
                                "Force basic 16-color, no-mouse".into(),
                                "TTY-compatible rendering.".into(),
                                "".into(),
                                "Set to True on a real Linux".into(),
                                "console (not a terminal".into(),
                                "emulator) with no 256/true".into(),
                                "color support.".into(),
                            ],
                            action: SettingsAction::ToggleFalseTty,
                        },
                        SettingsItem {
                            label: "Vim keys".into(),
                            value: bool_str(config.vim_keys),
                            description: vec![
                                "Enable vim keys.".into(),
                                "".into(),
                                "Set to True to enable".into(),
                                "\"j,k\" keys for directional".into(),
                                "control in lists, in addition".into(),
                                "to the arrow keys (which".into(),
                                "always work).".into(),
                            ],
                            action: SettingsAction::ToggleVimKeys,
                        },
                        SettingsItem {
                            label: "Disable mouse".into(),
                            value: bool_str(config.disable_mouse),
                            description: vec!["Disable all mouse events.".into()],
                            action: SettingsAction::ToggleMouse,
                        },
                        SettingsItem {
                            label: "Disable presets".into(),
                            value: bool_str(config.disable_presets),
                            description: vec![
                                "Hide the Presets entry below".into(),
                                "and disable cycling through".into(),
                                "saved zone layouts.".into(),
                            ],
                            action: SettingsAction::ToggleDisablePresets,
                        },
                        SettingsItem {
                            label: "Presets".into(),
                            value: if config.disable_presets {
                                "disabled".into()
                            } else {
                                preset_display
                            },
                            description: vec![
                                "Cycle through saved zone".into(),
                                "layouts (which panels are".into(),
                                "shown).".into(),
                                "".into(),
                                "Edit the `presets` list in".into(),
                                "config.toml to customize.".into(),
                            ],
                            action: SettingsAction::CyclePreset,
                        },
                        SettingsItem {
                            label: "Show boxes".into(),
                            value: bool_str(config.show_boxes),
                            description: vec![
                                "Show borders around panels.".into(),
                                "".into(),
                                "Set to False for a more".into(),
                                "minimal look with no borders.".into(),
                            ],
                            action: SettingsAction::ToggleShowBoxes,
                        },
                        SettingsItem {
                            label: "Update ms".into(),
                            value: config.update_ms.to_string(),
                            description: vec![
                                "Torrent panel refresh".into(),
                                "interval, in milliseconds.".into(),
                                "".into(),
                                "Min value: 100 ms".into(),
                                "Max value: 86400000 ms".into(),
                            ],
                            action: SettingsAction::SetUpdateMs,
                        },
                        SettingsItem {
                            label: "Rounded corners".into(),
                            value: bool_str(config.rounded_corners),
                            description: vec![
                                "Rounded corners on boxes.".into(),
                                "".into(),
                                "Is always False if False tty".into(),
                                "is On.".into(),
                            ],
                            action: SettingsAction::ToggleRoundedCorners,
                        },
                        SettingsItem {
                            label: "Terminal sync".into(),
                            value: bool_str(config.terminal_sync),
                            description: vec![
                                "Output synchronization.".into(),
                                "".into(),
                                "Use terminal synchronized".into(),
                                "output sequences to reduce".into(),
                                "flickering on supported".into(),
                                "terminals.".into(),
                            ],
                            action: SettingsAction::ToggleTerminalSync,
                        },
                        SettingsItem {
                            label: "Graph symbol".into(),
                            value: config.graph_symbol.clone(),
                            description: vec![
                                "Symbol set used for graphs".into(),
                                "and sparklines.".into(),
                                "".into(),
                                "\"braille\", \"block\" or \"dot\".".into(),
                            ],
                            action: SettingsAction::CycleGraphSymbol,
                        },
                        SettingsItem {
                            label: "Health check".into(),
                            value: "press Enter".into(),
                            description: vec![
                                "Run system health check.".into(),
                                "".into(),
                                "Verifies browser, TorrServer,".into(),
                                "saved credentials, cookies,".into(),
                                "and known sources.".into(),
                            ],
                            action: SettingsAction::RunHealthCheck,
                        },
                        SettingsItem {
                            label: "Save config on exit".into(),
                            value: bool_str(config.save_config_on_exit),
                            description: vec![
                                "Automatically save current".into(),
                                "settings to config.toml on".into(),
                                "exit.".into(),
                            ],
                            action: SettingsAction::ToggleSaveOnExit,
                        },
                    ],
                },
                SettingsCategory {
                    name: "streaming".into(),
                    items: streaming_settings_items(
                        config,
                        self.settings_browser_hidden,
                        &mode_str,
                    ),
                },
                SettingsCategory {
                    name: "download".into(),
                    items: vec![
                        SettingsItem {
                            label: "Enable downloading".into(),
                            value: bool_str(config.download_enabled),
                            description: vec![
                                "Allow \"Download\" play mode".into(),
                                "(saving .torrent files)".into(),
                                "in addition to streaming.".into(),
                            ],
                            action: SettingsAction::ToggleDownloadEnabled,
                        },
                        SettingsItem {
                            label: "Downloads directory".into(),
                            value: config.download_dir_mode.clone(),
                            description: vec![
                                "Which directory slot is".into(),
                                "active: \"default\" (OS".into(),
                                "Downloads folder) or".into(),
                                "custom1/2/3 below.".into(),
                            ],
                            action: SettingsAction::CycleDownloadDirMode,
                        },
                        SettingsItem {
                            label: "Custom directory 1".into(),
                            value: if config.download_dir_custom_1.is_empty() {
                                "(not set)".into()
                            } else {
                                config.download_dir_custom_1.clone()
                            },
                            description: vec![
                                "Edit `download_dir_custom_1`".into(),
                                "in config.toml.".into(),
                                "".into(),
                                "An in-app path editor is".into(),
                                "planned; not yet built.".into(),
                            ],
                            action: SettingsAction::Close,
                        },
                        SettingsItem {
                            label: "Custom directory 2".into(),
                            value: if config.download_dir_custom_2.is_empty() {
                                "(not set)".into()
                            } else {
                                config.download_dir_custom_2.clone()
                            },
                            description: vec![
                                "Edit `download_dir_custom_2`".into(),
                                "in config.toml.".into(),
                            ],
                            action: SettingsAction::Close,
                        },
                        SettingsItem {
                            label: "Custom directory 3".into(),
                            value: if config.download_dir_custom_3.is_empty() {
                                "(not set)".into()
                            } else {
                                config.download_dir_custom_3.clone()
                            },
                            description: vec![
                                "Edit `download_dir_custom_3`".into(),
                                "in config.toml.".into(),
                            ],
                            action: SettingsAction::Close,
                        },
                        SettingsItem {
                            label: "Close torrent core on exit".into(),
                            value: bool_str(config.close_torrent_core_on_exit),
                            description: vec![
                                "Stop the download when Doris".into(),
                                "exits, instead of leaving it".into(),
                                "running on the server.".into(),
                                "".into(),
                                "The torrent is paused, not".into(),
                                "removed: it stays on disk and".into(),
                                "resumes when asked for again.".into(),
                            ],
                            action: SettingsAction::ToggleCloseTorrentCoreOnExit,
                        },
                        SettingsItem {
                            label: "Open detailed log".into(),
                            value: "L".into(),
                            description: vec![
                                "Toggle detailed log view.".into(),
                                "".into(),
                                "Shows detailed application logs".into(),
                                "for debugging purposes.".into(),
                            ],
                            action: SettingsAction::OpenLog,
                        },
                    ],
                },
            ],
        });
    }

    pub fn settings_key(&mut self, key: crossterm::event::KeyEvent) -> Option<SettingsAction> {
        if let Modal::Settings(ref mut state) = self.modal {
            match key.code {
                crossterm::event::KeyCode::Esc => {
                    self.modal = Modal::None;
                    return Some(SettingsAction::Close);
                }
                crossterm::event::KeyCode::Char('j') | crossterm::event::KeyCode::Down => {
                    let cat = &state.categories[state.selected_category];
                    state.selected = (state.selected + 1).min(cat.items.len() - 1);
                    let page = state.selected / state.visible_items.max(1);
                    if page != state.page {
                        state.page = page;
                    }
                }
                crossterm::event::KeyCode::Char('k') | crossterm::event::KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                    let page = state.selected / state.visible_items.max(1);
                    if page != state.page {
                        state.page = page;
                    }
                }
                crossterm::event::KeyCode::Left => {
                    self.last_cycle_direction = -1;
                    let cat = &state.categories[state.selected_category];
                    if let Some(item) = cat.items.get(state.selected) {
                        return Some(item.action.clone());
                    }
                }
                crossterm::event::KeyCode::Right => {
                    self.last_cycle_direction = 1;
                    let cat = &state.categories[state.selected_category];
                    if let Some(item) = cat.items.get(state.selected) {
                        return Some(item.action.clone());
                    }
                }
                crossterm::event::KeyCode::Tab => {
                    state.selected_category =
                        (state.selected_category + 1) % state.categories.len();
                    state.selected = 0;
                    state.page = 0;
                }
                crossterm::event::KeyCode::BackTab => {
                    state.selected_category = if state.selected_category == 0 {
                        state.categories.len() - 1
                    } else {
                        state.selected_category - 1
                    };
                    state.selected = 0;
                    state.page = 0;
                }
                // Any digit 1-9 jumps to that category by position, not
                // just '1'/'2' -- this used to hardcode only the first two
                // categories, silently doing nothing for '3' once a third
                // ("download") category was added.
                crossterm::event::KeyCode::Char(c @ '1'..='9') => {
                    let idx = (c as u8 - b'1') as usize;
                    if idx < state.categories.len() {
                        state.selected_category = idx;
                        state.selected = 0;
                        state.page = 0;
                    }
                }
                crossterm::event::KeyCode::Enter => {
                    self.last_cycle_direction = 1;
                    let cat = &state.categories[state.selected_category];
                    if let Some(item) = cat.items.get(state.selected) {
                        return Some(item.action.clone());
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// The settings modal's own rendering: the descriptor table with
    /// its tab row, pagination and the item list. `&mut self` because
    /// the list is a `Selector` that mutates the modal state.
    pub fn render_settings_modal(&mut self, frame: &mut Frame, area: Rect, config: &Config) {
        if matches!(self.modal, Modal::Settings(_)) {
            let popup = centered_rect(80, 80, area);
            frame.render_widget(Clear, popup);

            let border_color = self.theme.primary_color();
            let main_block = self.modal_block(border_color, config);
            let inner = main_block.inner(popup);
            frame.render_widget(main_block, popup);

            let hi_color = self.theme.secondary_color();
            let title_color = self.theme.primary_color();
            // The glyphs that *are* keys (the paging arrows) take the
            // hover accent, the same rule the frame legend uses.
            let key_color = self.theme.on_hover_color();
            let div_color = self.theme.div_line.to_color();
            let fg_color = self.theme.main_fg.to_color();
            // The cursor row of this list is a selected row like any
            // other: same `selected_bg`/`selected_fg` Results, Trackers
            // and the detail modal's file list use, and the same btop
            // gives the option under the cursor (`btop_menu.cpp:1687`).
            let selection = self.theme.selection_style();

            if let Modal::Settings(ref mut state) = self.modal {
                let bw = inner.width as usize;
                let divider_col = SETTINGS_DIVIDER_COL.min(bw.saturating_sub(3));

                let tab_y = inner.y;
                let div_y = tab_y + 2;
                let content_y = div_y + 1;
                let content_h = inner.height.saturating_sub(4) as usize;

                // Slot width wide enough for every tab's label (works
                // regardless of how many categories exist or how long their
                // names are, instead of a hardcoded width that silently
                // corrupts once a name is long enough to fill it exactly --
                // see the bug this replaces, below).
                let slot_width = state
                    .categories
                    .iter()
                    .map(|cat| cat.name.chars().count() + 2)
                    .max()
                    .unwrap_or(8)
                    + 2; // breathing room before the next tab

                let mut tab_line = String::new();
                // (start, length, is_selected, mark offsets): the
                // marks are the characters that *are* the key -- the
                // digit that switches to an unselected tab, the
                // brackets around the selected one -- and take the
                // hover accent while the tab's name stays structure,
                // the split btop draws (`btop_menu.cpp:1631`).
                let mut tab_styles: Vec<(usize, usize, bool, Vec<usize>)> = Vec::new();
                let mut pos = 0;
                for (i, cat) in state.categories.iter().enumerate() {
                    let is_sel = i == state.selected_category;
                    let label = if is_sel {
                        format!("[{}]", cat.name)
                    } else {
                        format!("{}:{}", i + 1, cat.name)
                    };
                    let label_len = label.chars().count();
                    let marks = if is_sel {
                        vec![0, label_len - 1]
                    } else {
                        (0..(i + 1).to_string().len()).collect()
                    };
                    tab_styles.push((pos, label_len, is_sel, marks));
                    tab_line.push_str(&label);
                    for _ in label_len..slot_width {
                        tab_line.push(' ');
                    }
                    pos += slot_width;
                }

                // Bug fixed here: `pos` used to start at 2 while `ci` (the
                // actual index into `tab_line`'s characters) starts at 0, a
                // systematic 2-character offset between where each tab's
                // styling said it started and where its text actually was.
                // That caused this loop to both over-consume the previous
                // tab's trailing characters into the wrong style AND silently
                // drop the characters it skipped past to "catch up" -- which
                // is exactly the "[general] 2treaming3download" corruption
                // (missing the 's', tabs running together) from the bug
                // report. `pos` and `ci` now share the same coordinate space.
                let mut spans = Vec::new();
                let chars: Vec<char> = tab_line.chars().collect();
                let mut ci = 0;
                for (start, len, is_sel, marks) in &tab_styles {
                    while ci < chars.len() && ci < *start + *len {
                        let ch = chars[ci].to_string();
                        let style = if marks.contains(&(ci - *start)) {
                            Style::default().fg(key_color).add_modifier(Modifier::BOLD)
                        } else if *is_sel {
                            Style::default()
                                .fg(title_color)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(title_color)
                        };
                        spans.push(Span::styled(ch, style));
                        ci += 1;
                    }
                    while ci < chars.len() && ci < *start + slot_width {
                        ci += 1;
                    }
                }
                frame.render_widget(
                    Paragraph::new(Line::from(spans)),
                    Rect::new(inner.x, tab_y, inner.width, 1),
                );

                let mut div_spans: Vec<Span> = Vec::new();
                div_spans.push(Span::styled("├", Style::default().fg(hi_color)));
                for _ in 1..divider_col {
                    div_spans.push(Span::styled("─", Style::default().fg(div_color)));
                }
                div_spans.push(Span::styled("┬", Style::default().fg(hi_color)));
                for _ in divider_col + 1..bw.saturating_sub(1) {
                    div_spans.push(Span::styled("─", Style::default().fg(div_color)));
                }
                div_spans.push(Span::styled("┤", Style::default().fg(hi_color)));
                frame.render_widget(
                    Paragraph::new(Line::from(div_spans)),
                    Rect::new(inner.x, div_y, inner.width, 1),
                );

                for row in 0..content_h {
                    frame.render_widget(
                        Paragraph::new(Span::styled("│", Style::default().fg(div_color))),
                        Rect::new(inner.x + divider_col as u16, content_y + row as u16, 1, 1),
                    );
                }

                let visible_items = content_h / 2;
                // Fixes B2: settings_key() reads this exact number back, so
                // pagination can never desync from what's actually on screen.
                state.visible_items = visible_items.max(1);
                let cat = &state.categories[state.selected_category];
                let page = state.page;
                let start_idx = page * visible_items;

                let left_x = inner.x + 1;
                let right_x = inner.x + divider_col as u16 + 2;
                let right_w = bw.saturating_sub(divider_col + 3) as u16;

                for row_idx in 0..visible_items {
                    let item_idx = start_idx + row_idx;
                    let y = content_y + (row_idx * 2) as u16;

                    if item_idx < cat.items.len() {
                        let item = &cat.items[item_idx];
                        let is_sel = item_idx == state.selected;

                        let label = if is_sel {
                            // Fixes B1: this used to hardcode "3" regardless of
                            // the actual selected position.
                            //
                            // The theme row is the one row whose `n/m` is
                            // not about rows: there it counts themes, so
                            // that "which theme" has an answer (it used
                            // to print "1/15" forever -- the row's place
                            // in the category).
                            let suffix = match item.action {
                                SettingsAction::CycleTheme => match state.theme_pos {
                                    Some((n, total)) => format!("{n}/{total}"),
                                    None => format!("{}/{}", item_idx + 1, cat.items.len()),
                                },
                                _ => format!("{}/{}", item_idx + 1, cat.items.len()),
                            };
                            format!("{} {}", item.label, suffix)
                        } else {
                            item.label.clone()
                        };
                        let label_style = if is_sel {
                            selection.add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(title_color)
                        };
                        let centered_label = center_str(&label, divider_col - 2);
                        frame.render_widget(
                            Paragraph::new(Span::styled(centered_label, label_style)),
                            Rect::new(left_x, y, divider_col as u16 - 1, 1),
                        );

                        // btop lets the selection colours run onto the
                        // value line as well (no new colour is emitted
                        // for it), so the whole cursor row reads as one.
                        let val_style = if is_sel {
                            selection
                        } else {
                            Style::default().fg(fg_color)
                        };
                        let val_display = if is_sel {
                            format!("← {} →", item.value)
                        } else {
                            item.value.clone()
                        };
                        let centered_val = center_str(&val_display, divider_col - 2);
                        frame.render_widget(
                            Paragraph::new(Span::styled(centered_val, val_style)),
                            Rect::new(left_x, y + 1, divider_col as u16 - 1, 1),
                        );
                    }
                }

                if let Some(item) = cat.items.get(state.selected) {
                    let desc_style = Style::default().fg(fg_color);
                    for (i, line) in item.description.iter().enumerate() {
                        if (content_y as usize + i) < (content_y as usize + content_h) {
                            frame.render_widget(
                                Paragraph::new(Span::styled(line.as_str(), desc_style)),
                                Rect::new(right_x, content_y + i as u16, right_w, 1),
                            );
                        }
                    }
                }

                let pages = cat.items.len().div_ceil(visible_items);
                if pages > 1 {
                    let page_line = format!("↑ page {}/{} ↓", page + 1, pages);
                    let page_y = content_y + content_h as u16;
                    let page_x = inner.x + (bw / 2).saturating_sub(page_line.len() / 2) as u16;
                    frame.render_widget(
                        Paragraph::new(Line::from(vec![
                            Span::styled("┘", Style::default().fg(hi_color)),
                            Span::styled("↑ ", Style::default().fg(key_color)),
                            Span::styled(
                                format!("page {}/{} ", page + 1, pages),
                                Style::default().fg(title_color),
                            ),
                            Span::styled("↓", Style::default().fg(key_color)),
                            Span::styled("└", Style::default().fg(hi_color)),
                        ])),
                        Rect::new(
                            page_x.saturating_sub(1),
                            page_y,
                            page_line.len() as u16 + 4,
                            1,
                        ),
                    );
                }
            }
        }
    }
}
