//! Keyboard and mouse, and the frame each keypress redraws.
//!
//! Every key the app answers is routed from `handle_key`, which is the
//! whole of it: the guards for a detail view, a modal, the filter box and
//! the search box come first and each returns, so a key belongs to exactly
//! one mode.

use super::*;
use crate::ui::layout::Dir;
use ratatui::layout::Position;

/// Lines one notch of the mouse wheel moves.
pub(super) const MOUSE_SCROLL_STEP: i64 = 3;

impl App {
    pub(super) async fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.config.disable_mouse {
            return;
        }
        if self.ui.show_menu {
            // The menu swallows the rest of the mouse, so a click on an
            // item has to be read here -- and it was not, which left the
            // three items reachable only from the keyboard. A click on
            // Quit that does nothing is the one thing that reads as a
            // program that hung: the terminal is still full of it and
            // the browser it spawned is still burning CPU.
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                let area =
                    ratatui::layout::Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
                if let Some(idx) = crate::ui::menu::menu_item_rects(area)
                    .iter()
                    .position(|r| r.contains(Position::new(mouse.column, mouse.row)))
                {
                    self.ui.menu.selected = idx;
                    self.activate_menu_item().await;
                }
            }
            return;
        }

        match mouse.kind {
            MouseEventKind::Moved => {
                // Redrawn only when the hovered cell actually changed:
                self.ui.set_hover(mouse.row, mouse.column);
            }
            // One notch of the wheel, up or down. Both directions were
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let down = matches!(mouse.kind, MouseEventKind::ScrollDown);
                if self.ui.detail_view == Some(ZoneId::Log) {
                    self.ui.scroll_detail_log(if down {
                        MOUSE_SCROLL_STEP
                    } else {
                        -MOUSE_SCROLL_STEP
                    });
                } else if self.ui.modal == Modal::None {
                    if let Some(id) = self.ui.zone_at(mouse.row, mouse.column) {
                        self.ui.zones.focused = id;
                        match id {
                            ZoneId::Log => self.ui.scroll_logs(if down { 1 } else { -1 }),
                            ZoneId::Results => {
                                if down {
                                    self.handle_nav_down().await
                                } else {
                                    self.handle_nav_up()
                                }
                            }
                            // Torrent is a single status readout and
                            ZoneId::Torrent | ZoneId::Trackers => {}
                        }
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                // Same rule as the keyboard: a click that is not on the
                self.ui.disarm_remove();
                if self.ui.detail_view == Some(ZoneId::Log) {
                    self.ui.detail_log_scroll = self.ui.detail_logs.len();
                } else if self.ui.modal == Modal::None && self.ui.search_box_at(mouse.row) {
                    // The input box is the only thing left to hit on
                    self.ui.enter_input_mode();
                } else if self.ui.modal == Modal::None {
                    match self.ui.click_at(mouse.row, mouse.column, &mut self.config) {
                        Some(UiAction::TrackersChanged) => self.persist_config(),
                        Some(UiAction::ReaskCategory) => self.reask_for_category().await,
                        Some(UiAction::TogglePause) => self.toggle_pause_active_torrent().await,
                        Some(UiAction::Remove) => {
                            if self.ui.confirm_remove() {
                                self.remove_active_torrent().await;
                            }
                        }
                        Some(UiAction::Download) => {
                            self.download_selected_to_disk().await;
                        }
                        Some(UiAction::Info) => self.show_selected_info(),
                        Some(UiAction::Play) => {
                            // The `play` frame button is Enter on the
                            match enter_action(
                                false,
                                !self.ui.search_input.is_empty(),
                                self.ui.source_changed,
                                self.ui.group_changed,
                                self.ui.submit_selection().is_some(),
                            ) {
                                EnterAction::RestartSearch => {
                                    self.restart_search().await;
                                }
                                EnterAction::Play => self.spawn_stream().await,
                                EnterAction::SubmitQuery | EnterAction::DoNothing => {}
                            }
                        }
                        None => {}
                    }
                }
            }
            // The divider follow and the release: `resize_start` armed
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.ui.modal == Modal::None {
                    self.ui.zones.resize_drag(mouse.row, mouse.column);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => self.ui.zones.resize_end(),
            _ => {}
        }
    }

    /// Move the selection down in the focused zone, loading the next page of results if the
    /// Results zone just scrolled near its end.
    pub(super) async fn handle_nav_down(&mut self) {
        match self.ui.zones.focused {
            ZoneId::Results => {
                self.ui.navigate_down();
                if self.ui.needs_more() {
                    if let Some(q) = self.ui.search_query.clone() {
                        self.load_more(q).await;
                    }
                }
            }
            ZoneId::Log => self.ui.scroll_logs(1),
            // The Trackers panel is a list like the others, so the same
            ZoneId::Trackers => self.ui.navigate_trackers(1),
            // And so is the Torrents panel now that it is a list.
            ZoneId::Torrent => self.ui.navigate_downloads(1),
        }
    }

    /// Counterpart to [`handle_nav_down`](Self::handle_nav_down) for the Up
    /// arrow / vim-style 'k'.
    pub(super) fn handle_nav_up(&mut self) {
        match self.ui.zones.focused {
            ZoneId::Results => {
                self.ui.navigate_up();
            }
            ZoneId::Log => self.ui.scroll_logs(-1),
            ZoneId::Trackers => self.ui.navigate_trackers(-1),
            ZoneId::Torrent => self.ui.navigate_downloads(-1),
        }
    }

    /// `PageUp`/`PageDown` over the focused zone.
    async fn page_scrolled(&mut self, down: bool) {
        let step = crate::ui::view::LOG_PAGE_STEP as isize;
        match self.ui.zones.focused {
            ZoneId::Log => self.ui.scroll_logs(if down { step } else { -step }),
            ZoneId::Results => {
                self.ui.navigate_page(if down {
                    self.result_page()
                } else {
                    -self.result_page()
                });
            }
            ZoneId::Torrent | ZoneId::Trackers => {}
        }
    }

    /// One frame, bracketed by synchronized output when the option is on.
    pub(super) fn draw_frame(&mut self, terminal: &mut crate::tui::Terminal) -> Result<()> {
        if self.config.terminal_sync {
            crate::tui::begin_sync(terminal);
        }
        // The draw's result borrows the terminal (it hands back the frame
        let result = terminal
            .draw(|frame| {
                self.terminal_size = (frame.area().width, frame.area().height);
                self.ui.render(frame, &self.config);
            })
            .map(|_| ())
            .map_err(anyhow::Error::from);
        if self.config.terminal_sync {
            crate::tui::end_sync(terminal);
        }
        result?;
        Ok(())
    }

    /// How many rows one `PageUp`/`PageDown` covers in the Results panel: half the terminal,
    /// less the frame and the row the cursor has to stay visible in.
    pub(super) fn result_page(&self) -> isize {
        // Two rows for the frame, one so the cursor row is still on
        (self.terminal_size.1 / 2).saturating_sub(3).max(1) as isize
    }

    /// Re-run the query on screen, because the selection above it moved.
    pub(super) async fn restart_search(&mut self) {
        self.ui.source_changed = false;
        self.ui.group_changed = false;
        if let Some(query) = self.ui.search_query.clone() {
            self.start_search(query).await;
        }
    }

    pub(super) async fn handle_settings_key(&mut self, key: KeyEvent) {
        if let Some(action) = self.ui.settings_key(key) {
            // Captured before the loop flips it: turning TorrServer
            let torrserver_was_on = self.config.enable_torrserver;
            let toggled = apply_bool_toggle(&mut self.config, action);
            if toggled && self.config.enable_torrserver && !torrserver_was_on {
                // Turning TorrServer *on* is the one toggle that owes
                self.check_torrserver_on_enable().await;
            }
            match action {
                SettingsAction::ToggleBrowserVisibility => {
                    // `App::browser_visibility` is the single runtime
                    self.browser_visibility = match self.browser_visibility {
                        BrowserVisibility::Hidden => BrowserVisibility::Visible,
                        BrowserVisibility::Visible => BrowserVisibility::Hidden,
                    };
                }
                SettingsAction::ToggleMode => {
                    self.ui.stream_mode = !self.ui.stream_mode;
                }
                SettingsAction::CyclePrioritizeBrowser => {
                    const ORDER: &[&str] = &["helium", "brave", "chrome", "chromium"];
                    let current = self
                        .config
                        .browser_priority
                        .first()
                        .cloned()
                        .unwrap_or_default();
                    let next_first = cycle_str(&current, ORDER, self.ui.last_cycle_direction);
                    // Move next_first to the front, keep the rest in
                    let mut rest: Vec<String> = self
                        .config
                        .browser_priority
                        .iter()
                        .filter(|k| k.as_str() != next_first)
                        .cloned()
                        .collect();
                    let mut new_priority = vec![next_first.to_string()];
                    new_priority.append(&mut rest);
                    self.config.browser_priority = new_priority;
                }
                SettingsAction::EditCredentials => {
                    self.ui.open_login_modal();
                }
                SettingsAction::CheckTorrserverStatus => {
                    let reachable = self.torrserver.is_reachable().await;
                    let url = self.torrserver.base_url().to_string();
                    let msg = if reachable {
                        format!("TorrServer: reachable at {url}")
                    } else {
                        format!("TorrServer: not answering at {url}")
                    };
                    self.report("torrserver", &msg);
                }
                SettingsAction::OpenLog => {
                    self.ui.modal = Modal::None;
                    self.ui.detail_view = Some(ZoneId::Log);
                }
                SettingsAction::RunHealthCheck => {
                    let cookie_file = self.resolve_cookie_file();
                    let results = self.ui.health_check(cookie_file.as_deref()).await;
                    self.ui.modal = Modal::HealthCheck(results);
                }
                SettingsAction::CycleTheme => {
                    let themes = Theme::load_themes();
                    if let Some(pos) = themes.iter().position(|t| t.name == self.ui.theme.name) {
                        let next = cycle_index(pos, themes.len(), self.ui.last_cycle_direction);
                        self.ui.theme = themes[next].clone();
                    } else if !themes.is_empty() {
                        self.ui.theme = themes[0].clone();
                    }
                    self.config.theme_name = Some(self.ui.theme.name.clone());
                }
                SettingsAction::CyclePreset => {
                    self.cycle_layout_preset(self.ui.last_cycle_direction);
                }
                SettingsAction::SetUpdateMs => {
                    // No numeric text-entry widget exists in the
                    const STEPS: &[u64] = &[250, 500, 1000, 2000, 5000, 10000, 30000, 60000];
                    self.config.update_ms = match STEPS
                        .iter()
                        .position(|&v| v == self.config.update_ms)
                    {
                        Some(i) => STEPS[cycle_index(i, STEPS.len(), self.ui.last_cycle_direction)],
                        None => STEPS[0],
                    };
                }
                SettingsAction::CycleGraphSymbol => {
                    self.config.graph_symbol = cycle_str(
                        &self.config.graph_symbol,
                        &["braille", "block", "dot"],
                        self.ui.last_cycle_direction,
                    );
                }
                SettingsAction::CycleFileManager => {
                    self.config.file_manager = cycle_str(
                        &self.config.file_manager,
                        &crate::app::files::keys(),
                        self.ui.last_cycle_direction,
                    );
                }
                SettingsAction::CycleDownloadDirMode => {
                    self.config.download_dir_mode = cycle_str(
                        &self.config.download_dir_mode,
                        &["default", "custom1", "custom2", "custom3"],
                        self.ui.last_cycle_direction,
                    );
                }
                SettingsAction::ToggleWelcome => {
                    self.config.welcome_enabled = !self.config.welcome_enabled;
                }
                SettingsAction::CycleWelcomeTemplate => {
                    let names: Vec<String> = crate::welcome::load_templates()
                        .into_iter()
                        .map(|t| t.name)
                        .collect();
                    if !names.is_empty() {
                        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                        self.config.welcome_template = cycle_str(
                            &self.config.welcome_template,
                            &refs,
                            self.ui.last_cycle_direction,
                        );
                    }
                }
                SettingsAction::CycleWelcomeFrameMs => {
                    self.config.welcome_frame_ms = cycle_u64(
                        self.config.welcome_frame_ms,
                        &[20, 40, 60, 90, 120, 200, 400],
                        self.ui.last_cycle_direction,
                    );
                }
                SettingsAction::CycleWelcomeDurationMs => {
                    self.config.welcome_duration_ms = cycle_u64(
                        self.config.welcome_duration_ms,
                        &[0, 500, 1000, 1600, 3000, 6000, 15000],
                        self.ui.last_cycle_direction,
                    );
                }
                SettingsAction::EditWelcomeText => {
                    self.editing_welcome_text = Some(self.config.welcome_text.clone());
                }
                SettingsAction::Close => {}
                // The bool toggles are handled above by the
                _ => {}
            }

            // Rebuild the modal once, here, instead of nine times in
            if matches!(self.ui.modal, Modal::Settings(_)) {
                self.ui.open_settings(
                    &self.config,
                    self.browser_visibility == BrowserVisibility::Hidden,
                );
            }

            // Persist every settings change immediately rather than
            self.persist_config();
        }
    }

    /// How far a Log-view key should move the scroll: 1 for a line, `LOG_PAGE_STEP` for a page,
    /// down positive, `None` for a key that is not a scroll at all.
    fn log_scroll_step(&self, code: KeyCode) -> Option<i64> {
        let step = match code {
            KeyCode::Down => 1,
            KeyCode::Up => -1,
            KeyCode::PageDown => crate::ui::view::LOG_PAGE_STEP as i64,
            KeyCode::PageUp => -(crate::ui::view::LOG_PAGE_STEP as i64),
            KeyCode::Char('j') if self.config.vim_keys => 1,
            KeyCode::Char('k') if self.config.vim_keys => -1,
            _ => return None,
        };
        Some(step)
    }

    /// How far a Torrents detail key should move the cursor.
    ///
    /// A distance, not a destination, so `Home` and `End` are not here:
    /// they name a place, and adding one to a cursor that wraps does not
    /// land on it. `j`/`k` wrap like every other list in this app; `Home`
    /// and `End` are the two keys that go where they say.
    fn download_step(&self, code: KeyCode) -> Option<i64> {
        let page = (self.terminal_size.1 / 2).max(1) as i64;
        match code {
            KeyCode::Down => Some(1),
            KeyCode::Up => Some(-1),
            KeyCode::PageDown => Some(page),
            KeyCode::PageUp => Some(-page),
            KeyCode::Char('j') if self.config.vim_keys => Some(1),
            KeyCode::Char('k') if self.config.vim_keys => Some(-1),
            _ => None,
        }
    }

    /// Every mode that takes the keyboard away from the plain view, in the order they are asked
    /// about.
    async fn mode_owns_the_key(&mut self, key: KeyEvent) -> Result<Option<()>> {
        // Before the modal it was opened from: while a value is being
        // typed every other key means a character, including the ones the
        // Options modal would otherwise read as navigation.
        if self.editing_welcome_text.is_some() {
            self.welcome_text_key(key);
            return Ok(Some(()));
        }

        if self.ui.show_menu {
            self.handle_menu_key(key).await?;
            return Ok(Some(()));
        }

        if let Modal::HealthCheck(_) = self.ui.modal {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                self.ui.modal = Modal::None;
            }
            return Ok(Some(()));
        }

        if let Modal::Help(_) = self.ui.modal {
            // The help page owns the keyboard while it is up, exactly
            self.ui.help_key(key);
            return Ok(Some(()));
        }

        if let Modal::Files(_) = self.ui.modal {
            // The file list owns the keyboard: it is the only place a
            // download can be cut down to the files actually wanted.
            self.ui.files_key(key, self.config.vim_keys);
            self.send_pending_file_wants().await;
            return Ok(Some(()));
        }

        if let Modal::TorrentDetail(_) = self.ui.modal {
            // The detail modal owns the keyboard too: j/k move the file
            if let Some(action) = self.ui.detail_key(key, self.config.vim_keys) {
                match action {
                    DetailAction::Play => {
                        // Playing leaves the modal: the user is going
                        self.ui.modal = Modal::None;
                        self.spawn_stream().await;
                    }
                    DetailAction::Download => {
                        self.download_selected_to_disk().await;
                    }
                }
            }
            return Ok(Some(()));
        }

        if let Modal::Settings(_) = self.ui.modal {
            self.handle_settings_key(key).await;
        }

        // No `return` here: the check below is `modal != None`, which is

        if self.ui.modal != Modal::None {
            if let Some((resource, username, password)) = self.ui.login_modal_key(key) {
                self.do_login(resource, &username, &password).await;
            }
            return Ok(Some(()));
        }

        // A detail view owns the keyboard until it is dismissed -- but only
        // once no modal is up. Asked before the modals, Esc closed the
        // detail view *behind* an open modal and left the modal holding a
        // keyboard nothing reached it with: the file list opened over the
        // downloads, and Esc took the view away instead of the dialog.
        // A dialog is above everything; this one is a takeover, not one.
        if let Some(view) = self.ui.detail_view {
            // The zone's own key closes it, any other detail key jumps
            let target = match key.code {
                KeyCode::Char(c) => ZoneId::all()
                    .iter()
                    .copied()
                    .find(|id| id.detail_key() == Some(c)),
                _ => None,
            };
            match key.code {
                // Esc opens the menu instead of closing the view. A
                // detail view has its own key (`T`) and jumps to the other
                // two, so Esc closing it was a third way out that also threw
                // away the place you were -- and Esc means "step back", and
                // the step back from a full-frame takeover is the menu, the
                // same key the plain view uses.
                KeyCode::Esc => self.ui.show_menu = true,
                _ if target == Some(view) => self.ui.detail_view = None,
                _ if target.is_some() => self.ui.detail_view = target,
                // Only the Log view scrolls; the other two takeovers have
                _ if view == ZoneId::Log => {
                    if let Some(step) = self.log_scroll_step(key.code) {
                        self.ui.scroll_detail_log(step);
                    }
                }
                // The Torrents view is a list, so it moves a cursor and acts
                // on the row under it. It used to take the keyboard and do
                // nothing with it but close: a full-frame list of downloads
                // with no way to choose one of them.
                _ if view == ZoneId::Torrent => {
                    let last = self.ui.downloads.len().saturating_sub(1);
                    match key.code {
                        KeyCode::Home => self.ui.download_cursor = 0,
                        KeyCode::End => self.ui.download_cursor = last,
                        _ if self.download_step(key.code).is_some() => {
                            if let Some(step) = self.download_step(key.code) {
                                self.ui.navigate_downloads(step);
                            }
                        }
                        _ => self.torrent_detail_key(key.code).await?,
                    }
                }
                _ => {}
            }
            return Ok(Some(()));
        }

        if self.ui.zones.filter_mode {
            match key.code {
                KeyCode::Esc => {
                    self.ui.zones.filter_mode = false;
                    self.ui.zones.filter_input.clear();
                    self.ui.update_filter();
                }
                KeyCode::Enter => {
                    self.ui.zones.filter_mode = false;
                    self.ui.update_filter();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.ui.zones.filter_input.push(c);
                    self.ui.update_filter();
                }
                KeyCode::Backspace => {
                    self.ui.zones.filter_input.pop();
                    self.ui.update_filter();
                }
                _ => {}
            }
            return Ok(Some(()));
        }

        // The search box has no zone of its own, so "the Trackers panel is
        if self.ui.input_mode {
            self.handle_input_key(key).await?;
            return Ok(Some(()));
        }
        Ok(None)
    }

    async fn handle_plain_key(&mut self, key: KeyEvent) -> Result<()> {
        // The arrows carry the layout when they carry a modifier, and
        // they are handled before the `match` because that match reads
        // `key.code` -- which has already thrown the modifiers away.
        if Self::layout_arrow(&key) {
            let dir = Self::dir_of(key.code);
            match (
                key.modifiers.contains(KeyModifiers::CONTROL),
                key.modifiers.contains(KeyModifiers::SHIFT),
            ) {
                // Focus, swap, resize. Nothing plain: a bare arrow is
                // still the cursor.
                (true, true) => self.ui.zones.resize_focused(dir),
                (true, false) => self.ui.zones.focus_neighbour(dir),
                (false, true) => self.ui.zones.swap_focused(dir),
                (false, false) => false,
            };
            return Ok(());
        }

        match key.code {
            KeyCode::Char('m') => {
                self.ui.show_menu = !self.ui.show_menu;
            }
            KeyCode::Char('f') => {
                self.ui.zones.filter_mode = true;
            }
            KeyCode::Char('F') => {
                if self.ui.zones.fullscreen.is_some() {
                    self.ui.zones.set_fullscreen(None);
                } else {
                    self.ui.zones.set_fullscreen(Some(self.ui.zones.focused));
                }
            }
            // `1`-`4`, read through the same table the frame and the
            KeyCode::Char(c @ '1'..='4') => {
                if let Some(id) = ZoneId::from_key(c) {
                    self.ui.zones.focus_or_toggle(id);
                }
            }
            // Shift+P: cycle the layout presets, the same list the
            KeyCode::Char('P') => {
                self.cycle_layout_preset(1);
            }
            KeyCode::Char('p') if self.ui.zones.focused == ZoneId::Torrent => {
                // The Torrents panel is a list of what is being fetched, so
                // `p` and `d` act on that list. `active_torrent_hash` is
                // the streaming server's single torrent and stays where it
                // was, reached through the detail view.
                if self.ui.downloads.is_empty() {
                    self.toggle_pause_active_torrent().await;
                } else {
                    self.toggle_pause_download().await;
                }
            }
            KeyCode::Char('d') if self.ui.zones.focused == ZoneId::Torrent => {
                if !self.ui.downloads.is_empty() {
                    // Two presses, like the streaming panel's removal: the
                    // second one is what deletes what was fetched.
                    if self.ui.confirm_remove() {
                        self.remove_download().await;
                    }
                } else if self.ui.confirm_remove() {
                    self.remove_active_torrent().await;
                }
            }
            KeyCode::Char('d') if self.ui.zones.focused == ZoneId::Results => {
                self.download_selected_to_disk().await;
            }
            KeyCode::Char('v') if self.ui.zones.focused == ZoneId::Results => {
                self.show_selected_info();
            }
            // The category row's keys, next to `g`/`G` and gated the same
            KeyCode::Char('g') if self.ui.zones.focused == ZoneId::Results => {
                self.ui.cycle_group(true);
                self.reask_for_category().await;
            }
            KeyCode::Char('G') if self.ui.zones.focused == ZoneId::Results => {
                self.ui.cycle_group(false);
                self.reask_for_category().await;
            }
            // The Trackers panel's own keys: `j`/`k` move the cursor
            KeyCode::Char('j')
                if self.config.vim_keys && self.ui.zones.focused == ZoneId::Trackers =>
            {
                self.ui.navigate_trackers(1);
            }
            KeyCode::Char('k')
                if self.config.vim_keys && self.ui.zones.focused == ZoneId::Trackers =>
            {
                self.ui.navigate_trackers(-1);
            }
            KeyCode::Enter if self.ui.zones.focused == ZoneId::Trackers => {
                self.ui.toggle_source(&mut self.config);
                self.persist_config();
            }
            // Shift+Enter: the selected row's details. Separate
            KeyCode::Char('D') => {
                self.open_detail_modal().await;
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.open_detail_modal().await;
            }
            KeyCode::Char('j') if self.config.vim_keys => {
                self.handle_nav_down().await;
            }
            KeyCode::Down => {
                self.handle_nav_down().await;
            }
            KeyCode::Char('k') if self.config.vim_keys => {
                self.handle_nav_up();
            }
            KeyCode::Up => {
                self.handle_nav_up();
            }
            KeyCode::PageUp => {
                self.page_scrolled(false).await;
            }
            KeyCode::PageDown => {
                self.page_scrolled(true).await;
            }
            KeyCode::Tab => {
                self.ui.zones.focus_next();
            }
            KeyCode::BackTab => {
                self.ui.zones.focus_prev();
            }
            // Three keys for one box: `s` and `i` as they always were,
            KeyCode::Char('s') | KeyCode::Char('i') | KeyCode::Char('S') => {
                self.ui.enter_input_mode();
            }
            KeyCode::Char('b') => {
                // Browse: an empty query asks the browse-capable
                self.ui.source_changed = true;
                self.ui.set_group(None);
                self.start_search(String::new()).await;
            }
            KeyCode::Char('L') => {
                self.ui.toggle_detail_view(ZoneId::Log);
            }
            // The other two detail views: `T` the torrent's full
            KeyCode::Char('T') => {
                self.ui.toggle_detail_view(ZoneId::Torrent);
            }
            KeyCode::Char('R') => {
                self.ui.toggle_detail_view(ZoneId::Results);
            }
            // The help page (`F1`/`?`); `h` stays free
            KeyCode::Char('?') | KeyCode::Char('/') | KeyCode::F(1) => {
                self.ui.open_help_modal();
            }
            // detail log mode and input mode both returned above, so the
            KeyCode::Esc => {
                self.ui.show_menu = !self.ui.show_menu;
            }
            KeyCode::Enter => self.handle_enter().await,
            _ => {}
        }
        Ok(())
    }
    pub(super) async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        // An armed removal is a question waiting for an answer, and every
        self.ui.disarm_remove();
        // Quit first, always. Every mode below answers and returns
        if matches!(key.code, KeyCode::Char('q') | KeyCode::Char('c'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.ui.quit();
            return Ok(());
        }

        if self.mode_owns_the_key(key).await?.is_some() {
            return Ok(());
        }

        self.handle_plain_key(key).await
    }

    /// The greeting editor: Enter commits, Esc abandons, Backspace
    /// deletes, and anything else with a character is typed.
    ///
    /// Committing writes straight to the config and reopens the modal, so
    /// the row shows the new text without a restart -- though the
    /// animation itself still plays on the next launch, since it plays
    /// before this UI exists.
    fn welcome_text_key(&mut self, key: KeyEvent) {
        let Some(mut buf) = self.editing_welcome_text.take() else {
            return;
        };
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Enter => {
                self.config.welcome_text = buf;
                self.persist_config();
            }
            KeyCode::Backspace => {
                buf.pop();
                self.editing_welcome_text = Some(buf);
            }
            // Ctrl/Alt chords are commands elsewhere in the app, not text.
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                buf.push(c);
                self.editing_welcome_text = Some(buf);
            }
            _ => self.editing_welcome_text = Some(buf),
        }
        if self.editing_welcome_text.is_none() && matches!(self.ui.modal, Modal::Settings(_)) {
            self.ui.open_settings(
                &self.config,
                self.browser_visibility == BrowserVisibility::Hidden,
            );
        }
    }

    /// What Enter means on the main view: submit the typed query,
    /// re-search a selection that just changed, or play the highlighted row.
    pub(super) async fn handle_enter(&mut self) {
        let action = enter_action(
            self.ui.input_mode,
            !self.ui.search_input.is_empty(),
            self.ui.source_changed,
            self.ui.group_changed,
            self.ui.submit_selection().is_some(),
        );
        match action {
            EnterAction::SubmitQuery => {
                if let Some(query) = self.ui.submit_search() {
                    self.start_search(query).await;
                }
            }
            EnterAction::RestartSearch => {
                self.restart_search().await;
            }
            EnterAction::Play => self.spawn_stream().await,
            EnterAction::DoNothing => {}
        }
    }

    /// Every key typed into the search box, and nothing else: the box has no zone of its own,
    /// so `input_mode` can overlap any focused zone, and this is the single place that overlap
    /// is decided -- `handle_key` hands the key over before its own match runs.
    pub(super) async fn handle_input_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => self.ui.exit_input_mode(),
            KeyCode::Enter => self.handle_enter().await,
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ui.clear_input();
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ui.delete_word();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ui.type_char(c);
            }
            KeyCode::Backspace => self.ui.backspace(),
            _ => {}
        }
        Ok(())
    }

    pub(super) async fn handle_menu_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('m') | KeyCode::Esc => {
                self.ui.show_menu = false;
            }
            KeyCode::Char('j') if self.config.vim_keys => {
                self.ui.menu.next();
            }
            KeyCode::Down => {
                self.ui.menu.next();
            }
            KeyCode::Char('k') if self.config.vim_keys => {
                self.ui.menu.prev();
            }
            KeyCode::Up => {
                self.ui.menu.prev();
            }
            KeyCode::Tab => {
                self.ui.menu.next();
            }
            KeyCode::BackTab => {
                self.ui.menu.prev();
            }
            KeyCode::Enter => {
                self.activate_menu_item().await;
            }
            _ => {}
        }
        Ok(())
    }

    /// What the picked menu item does. Enter and a click on the item both
    /// land here, so the two cannot disagree about what Quit means.
    async fn activate_menu_item(&mut self) {
        match self.ui.menu.select() {
            MenuItem::Options => {
                // The menu item is Options; it opens the very same
                // Settings modal the item name says. It used to only
                // close the menu, which read as a dead key.
                self.ui.show_menu = false;
                self.ui.open_settings(
                    &self.config,
                    self.browser_visibility == BrowserVisibility::Hidden,
                );
            }
            MenuItem::Help => {
                // The same page `?` opens -- one help modal, two ways in.
                self.ui.show_menu = false;
                self.ui.open_help_modal();
            }
            MenuItem::Quit => self.ui.quit(),
        }
    }

    /// Whether `key` is an arrow carrying a layout modifier -- one of
    /// the four combinations that move, swap or resize panels. A bare
    /// arrow is left to the cursor.
    pub(super) fn layout_arrow(key: &KeyEvent) -> bool {
        matches!(
            key.code,
            KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right
        ) && (key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::SHIFT))
    }

    fn dir_of(code: KeyCode) -> Dir {
        match code {
            KeyCode::Up => Dir::Up,
            KeyCode::Down => Dir::Down,
            KeyCode::Left => Dir::Left,
            _ => Dir::Right,
        }
    }
}
