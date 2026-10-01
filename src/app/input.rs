//! Keyboard and mouse, and the frame each keypress redraws.
//!
//! Every key the app answers is routed from `handle_key`, which is the
//! whole of it: the guards for a detail view, a modal, the filter box and
//! the search box come first and each returns, so a key belongs to exactly
//! one mode.

use super::*;

/// Lines one notch of the mouse wheel moves. A wheel event carries no
/// count, so this is a choice, and it matches what the wheel does in
/// every other list on a desktop.
pub(super) const MOUSE_SCROLL_STEP: i64 = 3;

impl App {
    pub(super) async fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.config.disable_mouse {
            return;
        }
        if self.ui.show_menu {
            return;
        }

        match mouse.kind {
            MouseEventKind::Moved => {
                // Redrawn only when the hovered cell actually changed:
                // the terminal reports every movement, and a redraw per
                // movement would spend the CPU on frames that differ from
                // the last one.
                self.ui.set_hover(mouse.row, mouse.column);
            }
            // One notch of the wheel, up or down. Both directions were
            // separate arms writing the same match over the zones, and
            // they had already drifted: the up arm said what focusing
            // Torrent and Trackers means, the down arm said it again in
            // fewer words.
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
                            // Trackers scrolls its cursor rather than a
                            // list -- focusing them on hover is still
                            // correct, there's just no list to move
                            // within.
                            ZoneId::Torrent | ZoneId::Trackers => {}
                        }
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                // Same rule as the keyboard: a click that is not on the
                // remove button answers the armed question with "no".
                // Mouse clicks do not go through `handle_key`, so the
                // disarm that happens there has to happen here too.
                self.ui.disarm_remove();
                if self.ui.detail_view == Some(ZoneId::Log) {
                    self.ui.detail_log_scroll = self.ui.detail_logs.len();
                } else if self.ui.modal == Modal::None && self.ui.search_box_at(mouse.row) {
                    // The input box is the only thing left to hit on
                    // those rows: the header hints ("s: search | S:
                    // settings | ...") went with П.3, and clicking the
                    // field does what `s`/`i` do.
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
                        Some(UiAction::Download) => self.download_selected_to_disk().await,
                        Some(UiAction::Info) => self.show_selected_info(),
                        Some(UiAction::Play) => {
                            // The `play` frame button is Enter on the
                            // Results panel: same decision tree as the
                            // key, minus `input_mode` (a click can't have
                            // been typed into the search box).
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
            // it on the way down (a click on a border inside `click_at`),
            // so a drag that was never armed moves nothing -- and the
            // release always disarms, even after a click that never
            // moved.
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.ui.modal == Modal::None {
                    self.ui.zones.resize_drag(mouse.row, mouse.column);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => self.ui.zones.resize_end(),
            _ => {}
        }
    }

    /// Move the selection down in the focused zone, loading the next page
    /// of results if the Results zone just scrolled near its end. Shared
    /// by the Down arrow (always active) and the vim-style 'j' (only when
    /// `config.vim_keys` is on) -- see `handle_key`.
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
            // keys move its cursor -- the one piece of state it has.
            ZoneId::Trackers => self.ui.navigate_trackers(1),
            _ => {}
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
            _ => {}
        }
    }

    /// One frame, bracketed by synchronized output when the option is on.
    ///
    /// The sequence brackets the frame and nothing else: turning it on
    /// for the whole run would leave the terminal buffering through the
    /// wait between frames, where there is nothing to present, and the
    /// app would look frozen.
    ///
    /// `?2026h` with no `?2026l` leaves the terminal *buffering* -- the
    /// app then looks frozen with no error and no way out but killing it
    /// -- so the closing sequence is written whatever `draw` returned.
    /// The two `backend_mut()` borrows are separate on purpose: holding
    /// a guard across `draw` would be the borrow error above, and the
    /// borrow checker is what guarantees the closing write is not skipped.
    pub(super) fn draw_frame(&mut self, terminal: &mut crate::tui::Terminal) -> Result<()> {
        if self.config.terminal_sync {
            crate::tui::begin_sync(terminal);
        }
        // The draw's result borrows the terminal (it hands back the frame
        // it completed), so it is unwrapped to an owned `Result` before
        // the closing sequence is written -- otherwise the borrow would
        // still be live and the terminal could not be touched again.
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

    /// How many rows one `PageUp`/`PageDown` covers in the Results
    /// panel: half the terminal, less the frame and the row the cursor
    /// has to stay visible in. A page the size of the whole window would
    /// put the selection off the bottom on the way back.
    pub(super) fn result_page(&self) -> isize {
        // Two rows for the frame, one so the cursor row is still on
        // screen. `saturating_sub` rather than a clamp: on a terminal
        // too short to page, a page of zero would make the key do
        // nothing at all, which is what it did before.
        (self.terminal_size.1 / 2).saturating_sub(3).max(1) as isize
    }

    /// Re-run the query on screen, because the selection above it moved.
    ///
    /// Reached from Enter and from a click on the table, which are the
    /// same question asked twice -- the block used to be written out
    /// twice, and a fix that reached one of them would have been invisible
    /// in the other.
    ///
    /// Both flags clear here as well as in `start_search`, for the only
    /// path where no search follows: nothing has ever been searched, so
    /// there is no query to restart and no row to play either.
    pub(super) async fn restart_search(&mut self) {
        self.ui.source_changed = false;
        self.ui.group_changed = false;
        if let Some(query) = self.ui.search_query.clone() {
            self.start_search(query).await;
        }
    }

    /// One keypress inside the Options modal.
    ///
    /// Out of `handle_key` because that function is otherwise the whole
    /// keyboard, and a modal's routing is a question with its own
    /// answer: which row does this key land on, what does that row do,
    /// and does anything else own the keyboard instead. The key never
    /// comes back out to the main view -- the modal owns the keyboard
    /// while it is up -- so there is nothing to unwind here, which is
    /// why this returns rather than `Result`.
    pub(super) async fn handle_settings_key(&mut self, key: KeyEvent) {
        if let Some(action) = self.ui.settings_key(key) {
            // Captured before the loop flips it: turning TorrServer
            // *on* is the one toggle that owes the user an answer.
            let torrserver_was_on = self.config.enable_torrserver;
            let toggled = apply_bool_toggle(&mut self.config, action);
            if toggled && self.config.enable_torrserver && !torrserver_was_on {
                // Turning TorrServer *on* is the one toggle that owes
                // the user an answer. It only writes to the log, so it
                // is safe before the modal is rebuilt.
                self.check_torrserver_on_enable().await;
            }
            match action {
                SettingsAction::ToggleBrowserVisibility => {
                    // `App::browser_visibility` is the single runtime
                    // owner; the modal reads a session copy on open.
                    // Previously this toggle only updated the display
                    // label and had zero effect on the next launch
                    // .
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
                    let next_first = match ORDER.iter().position(|&k| k == current) {
                        Some(i) => ORDER[cycle_index(i, ORDER.len(), self.ui.last_cycle_direction)],
                        None => ORDER[0],
                    };
                    // Move next_first to the front, keep the rest in
                    // their existing relative order.
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
                    // Settings modal yet, so this cycles through a
                    // fixed set of sensible intervals -- same
                    // interaction pattern as Color theme/Presets/Graph
                    // symbol above. A free-form numeric input is a
                    // reasonable follow-up once the modal supports one.
                    const STEPS: &[u64] = &[250, 500, 1000, 2000, 5000, 10000, 30000, 60000];
                    let next = match STEPS.iter().position(|&v| v == self.config.update_ms) {
                        Some(i) => STEPS[cycle_index(i, STEPS.len(), self.ui.last_cycle_direction)],
                        None => STEPS[0],
                    };
                    self.config.update_ms = next;
                }
                SettingsAction::CycleGraphSymbol => {
                    const SYMBOLS: &[&str] = &["braille", "block", "dot"];
                    let next = match SYMBOLS.iter().position(|&s| s == self.config.graph_symbol) {
                        Some(i) => {
                            SYMBOLS[cycle_index(i, SYMBOLS.len(), self.ui.last_cycle_direction)]
                        }
                        None => SYMBOLS[0],
                    };
                    self.config.graph_symbol = next.to_string();
                }
                SettingsAction::CycleDownloadDirMode => {
                    const MODES: &[&str] = &["default", "custom1", "custom2", "custom3"];
                    let next = match MODES
                        .iter()
                        .position(|&m| m == self.config.download_dir_mode)
                    {
                        Some(i) => MODES[cycle_index(i, MODES.len(), self.ui.last_cycle_direction)],
                        None => MODES[0],
                    };
                    self.config.download_dir_mode = next.to_string();
                }
                SettingsAction::Close => {}
                // The bool toggles are handled above by the
                // `BOOL_TOGGLES` table; nothing else to do here.
                _ => {}
            }

            // Rebuild the modal once, here, instead of nine times in
            // the arms above -- all of them were this same call. The
            // guard matters: three of those arms (`EditCredentials`,
            // `RunHealthCheck`, `OpenLog`) replace the modal or the
            // whole view, and rebuilding Options on top of the login
            // or health window would put it back.
            if matches!(self.ui.modal, Modal::Settings(_)) {
                self.ui.open_settings(
                    &self.config,
                    self.browser_visibility == BrowserVisibility::Hidden,
                );
            }

            // Persist every settings change immediately rather than
            // only on a clean exit ("Save config on exit" governs a
            // final flush, not whether changes are remembered at all
            // -- a crash between now and exit shouldn't lose them,
            // and it previously did).
            self.persist_config();
        }
    }

    /// How far a Log-view key should move the scroll: 1 for a line,
    /// `LOG_PAGE_STEP` for a page, down positive, `None` for a key that
    /// is not a scroll at all.
    ///
    /// The vim letters are gated on the setting here rather than in the
    /// match, so `j` scrolls exactly when `Down` does.
    fn log_scroll_step(&self, code: KeyCode) -> Option<i64> {
        let step = match code {
            KeyCode::Down => 1,
            KeyCode::Up => -1,
            KeyCode::PageDown => crate::ui::app::LOG_PAGE_STEP as i64,
            KeyCode::PageUp => -(crate::ui::app::LOG_PAGE_STEP as i64),
            KeyCode::Char('j') if self.config.vim_keys => 1,
            KeyCode::Char('k') if self.config.vim_keys => -1,
            _ => return None,
        };
        Some(step)
    }

    pub(super) async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        // An armed removal is a question waiting for an answer, and every
        // key that is not `d` is a "no". Disarmed here, before any mode
        // below reads the key, so no path that handles a key can miss the
        // cancellation.
        self.ui.disarm_remove();
        // Quit first, always. Every mode below answers and returns
        // before the plain-view match is reached -- the menu, a modal, a
        // detail view, the search box -- so a Ctrl+C arm at the bottom
        // of a match is a Ctrl+C that works in exactly one of them.
        // Ctrl+Q quits the same way (btop's quit is `q`).
        if matches!(key.code, KeyCode::Char('q') | KeyCode::Char('c'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.ui.quit();
            return Ok(());
        }

        if self.ui.show_menu {
            return self.handle_menu_key(key).await;
        }

        // A detail view owns the keyboard until it is dismissed: no
        // zone digits, no search box, no menu -- only the keys it
        // answers. Esc closes it, its own key closes it, and the other
        // two detail keys switch straight to that view instead.
        if let Some(view) = self.ui.detail_view {
            // The zone's own key closes it, any other detail key jumps
            // straight to that view -- a takeover you have to walk back
            // out of one at a time is a trap, not a mode.
            let target = match key.code {
                KeyCode::Char(c) => ZoneId::all()
                    .iter()
                    .copied()
                    .find(|id| id.detail_key() == Some(c)),
                _ => None,
            };
            match key.code {
                KeyCode::Esc => self.ui.detail_view = None,
                _ if target == Some(view) => self.ui.detail_view = None,
                _ if target.is_some() => self.ui.detail_view = target,
                // Only the Log view scrolls; the other two takeovers have
                // no list to move within. Six arms used to say that,
                // three of them writing the same clamped arithmetic, and
                // the page step a bare `20` next to a `LOG_PAGE_STEP`
                // that already exists.
                _ if view == ZoneId::Log => {
                    if let Some(step) = self.log_scroll_step(key.code) {
                        self.ui.scroll_detail_log(step);
                    }
                }
                _ => {}
            }
            return Ok(());
        }

        if let Modal::HealthCheck(_) = self.ui.modal {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                self.ui.modal = Modal::None;
            }
            return Ok(());
        }

        if let Modal::Help(_) = self.ui.modal {
            // The help page owns the keyboard while it is up, exactly
            // like btop's `helpMenu` -- every key lands here.
            self.ui.help_key(key);
            return Ok(());
        }

        if let Modal::TorrentDetail(_) = self.ui.modal {
            // The detail modal owns the keyboard too: j/k move the file
            // cursor, Enter plays, `d` downloads, Esc/q close. The two
            // actions that belong to the orchestrator come back.
            if let Some(action) = self.ui.detail_key(key, self.config.vim_keys) {
                match action {
                    DetailAction::Play => {
                        // Playing leaves the modal: the user is going
                        // to watch the torrent, not read about it.
                        self.ui.modal = Modal::None;
                        self.spawn_stream().await;
                    }
                    DetailAction::Download => self.download_selected_to_disk().await,
                }
            }
            return Ok(());
        }

        if let Modal::Settings(_) = self.ui.modal {
            self.handle_settings_key(key).await;
        }

        // No `return` here: the check below is `modal != None`, which is
        // still true for the Options window, so it returns for us. An
        // earlier version had both and the second one was unreachable.

        if self.ui.modal != Modal::None {
            if let Some((resource, username, password)) = self.ui.login_modal_key(key) {
                self.do_login(resource, &username, &password).await;
            }
            return Ok(());
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
            return Ok(());
        }

        // The search box has no zone of its own, so "the Trackers panel is
        // focused" and "a query is being typed" are not mutually
        // exclusive. Every arm below therefore needs `!input_mode` to stay
        // out of the user's way -- and that is exactly the guard that kept
        // being left off (`j`/`k` on the panel, `j`/`k` and Up/Down
        // everywhere). Typing owns the key here instead, so one check
        // replaces a guard every future arm would have to remember.
        if self.ui.input_mode {
            return self.handle_input_key(key).await;
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
            KeyCode::Char('1') => {
                self.ui.zones.focus_or_toggle(ZoneId::Results);
            }
            KeyCode::Char('2') => {
                self.ui.zones.focus_or_toggle(ZoneId::Torrent);
            }
            KeyCode::Char('3') => {
                self.ui.zones.focus_or_toggle(ZoneId::Trackers);
            }
            KeyCode::Char('4') => {
                self.ui.zones.focus_or_toggle(ZoneId::Log);
            }
            // Shift+P: cycle the layout presets (П.8), the same list the
            // Options row cycles -- one list, not two. Lowercase `p` is
            // pause/resume on the Torrent panel, so the capital is the
            // free one -- the same reasoning as `F` for filter.
            KeyCode::Char('P') => {
                self.cycle_layout_preset(1);
            }
            KeyCode::Char('p') if self.ui.zones.focused == ZoneId::Torrent => {
                self.toggle_pause_active_torrent().await;
            }
            KeyCode::Char('d') if self.ui.zones.focused == ZoneId::Torrent => {
                if self.ui.confirm_remove() {
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
            // way: `g` steps forward, `G` (shift) back. Both move the row
            // *and* re-ask the sources for it -- a category is a request,
            // not only a view (see `reask_for_category`).
            KeyCode::Char('g') if self.ui.zones.focused == ZoneId::Results => {
                self.ui.cycle_group(true);
                self.reask_for_category().await;
            }
            KeyCode::Char('G') if self.ui.zones.focused == ZoneId::Results => {
                self.ui.cycle_group(false);
                self.reask_for_category().await;
            }
            // The Trackers panel's own keys: `j`/`k` move the cursor
            // (wrapping, like every other list in the app), Enter switches
            // the row under it. Both are gated on the panel being focused
            // for the same reason `g`/`G` are gated on Results -- a key
            // that moved a cursor somewhere the user is not looking would
            // be a surprise. Typing is already off the table by the time
            // this match runs (see the `input_mode` hand-over above), so
            // none of the three needs its own half of that guard.
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
            // Shift+Enter: the selected row's details (П.7). Separate
            // from the plain Enter below on purpose -- that one plays
            // or re-searches, and a modifier is the only thing that can
            // tell the two apart.
            //
            // `D` is the fallback: most terminals send Shift+Enter as a
            // plain Enter with no modifier (crossterm only reports the
            // shift when the terminal opts into the kitty keyboard
            // protocol, which 0.28 has no API to request), so on those
            // Shift+Enter falls through to play and the modal never
            // opens. `D` is the same action on a key every terminal
            // sends distinctly.
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
            KeyCode::Tab => {
                self.ui.zones.focus_next();
            }
            KeyCode::BackTab => {
                self.ui.zones.focus_prev();
            }
            KeyCode::PageUp => {
                match self.ui.zones.focused {
                    ZoneId::Log => {
                        self.ui
                            .scroll_logs(-(crate::ui::app::LOG_PAGE_STEP as isize));
                    }
                    ZoneId::Results => {
                        self.ui.navigate_page(-self.result_page());
                    }
                    // Torrent is a status readout and Trackers is ten
                    // rows: neither has a page to turn.
                    ZoneId::Torrent | ZoneId::Trackers => {}
                }
            }
            KeyCode::PageDown => match self.ui.zones.focused {
                ZoneId::Log => {
                    self.ui.scroll_logs(crate::ui::app::LOG_PAGE_STEP as isize);
                }
                ZoneId::Results => {
                    self.ui.navigate_page(self.result_page());
                }
                ZoneId::Torrent | ZoneId::Trackers => {}
            },
            // Three keys for one box: `s` and `i` as they always were,
            // plus `S` -- Settings moved to the menu, and the letter
            // this app's users already had under their pinky keeps
            // working as "start typing".
            KeyCode::Char('s') | KeyCode::Char('i') | KeyCode::Char('S') => {
                self.ui.enter_input_mode();
            }
            KeyCode::Char('b') => {
                // Browse (B9): an empty query asks the browse-capable
                // sources for their freshest rows. Browse is cross-source
                // by nature, so it takes the "all" category with it -- a
                // mixed list of rows claiming no group must stay visible.
                self.ui.source_changed = true;
                self.ui.set_group(None);
                self.start_search(String::new()).await;
            }
            KeyCode::Char('L') => {
                self.ui.toggle_detail_view(ZoneId::Log);
            }
            // The other two detail views: `T` the torrent's full
            // readout, `R` the results table with its preview line.
            KeyCode::Char('T') => {
                self.ui.toggle_detail_view(ZoneId::Torrent);
            }
            KeyCode::Char('R') => {
                self.ui.toggle_detail_view(ZoneId::Results);
            }
            // The help page (btop binds `F1`/`?`/`h`); `h` stays free
            // for future vim navigation, so the three triggers are `?`,
            // `/` and F1.
            KeyCode::Char('?') | KeyCode::Char('/') | KeyCode::F(1) => {
                self.ui.open_help_modal();
            }
            // detail log mode and input mode both returned above, so the
            // only Esc left to answer for is the main view's.
            KeyCode::Esc => {
                self.ui.show_menu = !self.ui.show_menu;
            }
            KeyCode::Enter => self.handle_enter().await,
            _ => {}
        }
        Ok(())
    }

    /// What Enter means on the main view (btop's `enter`/`play`): submit
    /// the typed query, re-search a selection that just changed, or play
    /// the highlighted row. Also the key input mode passes it to --
    /// [`handle_input_key`] calls it with `input_mode` still on, where
    /// `enter_action` can only answer SubmitQuery or DoNothing.
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

    /// Every key typed into the search box, and nothing else: the box
    /// has no zone of its own, so `input_mode` can overlap any focused
    /// zone, and this is the single place that overlap is decided --
    /// `handle_key` hands the key over before its own match runs.
    ///
    /// Esc and Enter go through the same paths they have on any other
    /// view (leave input mode / the Enter decision tree); quitting stays
    /// here so Ctrl+C works while a query is being typed.
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
                let item = self.ui.menu.select();
                match item {
                    MenuItem::Options => {
                        self.ui.show_menu = false;
                    }
                    MenuItem::Help => {
                        // The same page `?` opens -- one help modal, two
                        // ways to reach it -- and the menu goes away with
                        // it, like Options does for the Settings modal.
                        self.ui.show_menu = false;
                        self.ui.open_help_modal();
                    }
                    MenuItem::Quit => {
                        self.ui.quit();
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
