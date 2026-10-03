//! The settings modal (the largest of the three): the typed descriptor
//! table the Options modal renders, its
//! pagination and key handling, and the per-category item builders
//! (general/streaming/download plus the derived sources list). The
//! `App` struct and the `Modal` enum stay in `ui/app.rs`; this file is
//! `impl App` blocks for just this modal's piece, the same pattern
//! `ui/modals/login.rs` and `health.rs` use.

use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui::Frame;

use super::settings_items::settings_categories;
use crate::config::Config;
use crate::sources::source::{Group, GROUP_ORDER, KNOWN_SOURCES};
use crate::ui::view::{centered_rect, App, Modal};

/// Column where the settings modal draws its vertical divider between the option name and its
/// value.
const SETTINGS_DIVIDER_COL: usize = 30;

impl SettingsState {
    pub fn follow_page(&mut self) {
        let page = self.selected / self.visible_items.max(1);
        if page != self.page {
            self.page = page;
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsState {
    pub selected_category: usize,
    pub selected: usize,
    pub page: usize,
    /// Items shown per page, computed from the real terminal size the last time this modal was
    /// rendered.
    pub visible_items: usize,
    pub categories: Vec<SettingsCategory>,
    /// `(index, total)` of the *theme* the settings show, not of the row that shows them: the
    /// one number on this modal that answers "which one am I on" for a value with an order of
    /// its own.
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

#[derive(Clone, Copy, Debug, PartialEq)]
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
    /// Which file manager `o` opens a download in.
    CycleFileManager,
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
    ToggleWelcome,
    CycleWelcomeTemplate,
    CycleWelcomeFrameMs,
    CycleWelcomeDurationMs,
    EditWelcomeText,
    Close,
}

pub(super) fn bool_str(b: bool) -> String {
    if b {
        "True".into()
    } else {
        "False".into()
    }
}

/// The category row's tabs: "all", then -- in `GROUP_ORDER` -- every group that at least one
/// enabled, implemented source serves.
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

/// The Options "streaming" category's items, built from the config the same way [`group_tabs`]
/// builds the category row -- one place, so a test can assert what the category offers without
/// a rendered modal.
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

/// The Options "welcome" category's items.
///
/// The values shown here are what the *next* launch will use: the
/// greeting plays before the UI exists, so there is nothing on screen to
/// preview it in and a setting that only takes effect on the next start
/// has to say so rather than look broken.
pub fn welcome_settings_items(config: &Config) -> Vec<SettingsItem> {
    vec![
        SettingsItem {
            label: "Welcome animation".into(),
            value: bool_str(config.welcome_enabled),
            description: vec![
                "Play an ASCII greeting".into(),
                "before the UI comes up.".into(),
                "".into(),
                "Applies on the next launch:".into(),
                "the greeting plays before".into(),
                "there is a UI to change it".into(),
                "from.".into(),
            ],
            action: SettingsAction::ToggleWelcome,
        },
        SettingsItem {
            label: "Welcome template".into(),
            value: config.welcome_template.clone(),
            description: vec![
                "Which animation to play.".into(),
                "".into(),
                "\"doris\" ships with the app.".into(),
                "Any .anim file in".into(),
                "\"~/.config/doris/welcome\" is".into(),
                "yours to add; one named".into(),
                "after a built-in replaces".into(),
                "it.".into(),
            ],
            action: SettingsAction::CycleWelcomeTemplate,
        },
        SettingsItem {
            label: "Welcome speed".into(),
            value: format!("{} ms/frame", config.welcome_frame_ms),
            description: vec![
                "Milliseconds between frames.".into(),
                "".into(),
                "Lower is faster. Duration".into(),
                "rounds up to a whole run,".into(),
                "so the last frame is never".into(),
                "cut off short.".into(),
            ],
            action: SettingsAction::CycleWelcomeFrameMs,
        },
        SettingsItem {
            label: "Welcome duration".into(),
            value: format!("{} ms", config.welcome_duration_ms),
            description: vec![
                "How long the animation plays.".into(),
                "".into(),
                "The frames repeat to fill it.".into(),
                "Zero plays it once through,".into(),
                "however long that takes.".into(),
            ],
            action: SettingsAction::CycleWelcomeDurationMs,
        },
        SettingsItem {
            label: "Welcome text".into(),
            value: if config.welcome_text.is_empty() {
                "(empty)".to_string()
            } else {
                config.welcome_text.clone()
            },
            description: vec![
                "The greeting itself.".into(),
                "".into(),
                "It appears wherever the".into(),
                "template wrote {text}.".into(),
                "".into(),
                "press Enter to edit, Esc to".into(),
                "leave it as it was.".into(),
            ],
            action: SettingsAction::EditWelcomeText,
        },
    ]
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
    pub fn open_settings(&mut self, config: &Config, browser_hidden: bool) {
        self.settings_browser_hidden = browser_hidden;
        let mode_str = if self.stream_mode {
            "Streaming (TorrServer)".to_string()
        } else {
            "Download (.torrent file)".to_string()
        };
        let theme_name = self.theme.name.clone();
        // Value shown between the cycle arrows must be the theme's own
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
            categories: settings_categories(
                config,
                browser_hidden,
                mode_str,
                theme_str,
                preset_display,
            ),
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
                    state.follow_page();
                }
                crossterm::event::KeyCode::Char('k') | crossterm::event::KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                    state.follow_page();
                }
                // The three keys that act *on* the item under the cursor
                crossterm::event::KeyCode::Left
                | crossterm::event::KeyCode::Right
                | crossterm::event::KeyCode::Enter => {
                    self.last_cycle_direction = if key.code == crossterm::event::KeyCode::Left {
                        -1
                    } else {
                        1
                    };
                    let cat = &state.categories[state.selected_category];
                    if let Some(item) = cat.items.get(state.selected) {
                        return Some(item.action);
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
                crossterm::event::KeyCode::Char(c @ '1'..='9') => {
                    let idx = (c as u8 - b'1') as usize;
                    if idx < state.categories.len() {
                        state.selected_category = idx;
                        state.selected = 0;
                        state.page = 0;
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// The settings modal's own rendering: the descriptor table with its tab row, pagination
    /// and the item list.
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
            let key_color = self.theme.hi_fg.to_color();
            let div_color = self.theme.div_line.to_color();
            let fg_color = self.theme.main_fg.to_color();
            // The cursor row of this list is a selected row like any
            let selection = self.theme.selection_style();

            if let Modal::Settings(ref mut state) = self.modal {
                let bw = inner.width as usize;
                let divider_col = SETTINGS_DIVIDER_COL.min(bw.saturating_sub(3));

                let tab_y = inner.y;
                let div_y = tab_y + 2;
                let content_y = div_y + 1;
                let content_h = inner.height.saturating_sub(4) as usize;

                // Slot width wide enough for every tab's label (works
                let slot_width = state
                    .categories
                    .iter()
                    .map(|cat| cat.name.chars().count() + 2)
                    .max()
                    .unwrap_or(8)
                    + 2; // breathing room before the next tab

                let mut tab_line = String::new();
                // (start, length, is_selected, mark offsets): the
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
                // settings_key() reads this exact number back, so
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
                            // this used to hardcode "3" regardless of
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

                        // The selection colours run onto the
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
