//! The login modal: the resource tabs,
//! the username/password fields, the saved-indicator and the Ctrl+S
//! save-without-login. Extracted from `ui/app.rs` so that file stops
//! being the whole UI in one place; this is the modal's own state and
//! rendering, nothing else.

use ratatui::layout::{Constraint, Direction, Rect};
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::config::Config;
use crate::ui::view::{centered_rect, App, Modal};

#[derive(Clone, Debug, PartialEq)]
pub struct LoginState {
    pub username: String,
    pub password: String,
    pub focus: LoginField,
    /// Which resource tab is selected: the id the entered credentials belong to, and the one
    /// Ctrl+S saves and the saved-indicator reads.
    pub resource: &'static str,
    pub message: Option<String>,
}

#[derive(PartialEq, Clone, Debug)]
pub enum LoginField {
    Username,
    Password,
}

impl Default for LoginState {
    fn default() -> Self {
        Self::new()
    }
}

impl LoginState {
    pub fn new() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            focus: LoginField::Username,
            resource: crate::credentials::LOGIN_RESOURCES[0],
            message: None,
        }
    }
}

impl App {
    pub fn open_login_modal(&mut self) {
        self.modal = Modal::Login(LoginState::new());
    }

    pub fn close_login_modal(&mut self) {
        self.modal = Modal::None;
    }

    pub fn login_modal_key(
        &mut self,
        key: crossterm::event::KeyEvent,
    ) -> Option<(&'static str, String, String)> {
        if let Modal::Login(ref mut state) = self.modal {
            match key.code {
                crossterm::event::KeyCode::Esc => {
                    self.modal = Modal::None;
                    return None;
                }
                crossterm::event::KeyCode::Left | crossterm::event::KeyCode::Right => {
                    let resources = crate::credentials::LOGIN_RESOURCES;
                    let current = resources
                        .iter()
                        .position(|&r| r == state.resource)
                        .unwrap_or(0);
                    let next = if key.code == crossterm::event::KeyCode::Right {
                        (current + 1) % resources.len()
                    } else {
                        (current + resources.len() - 1) % resources.len()
                    };
                    state.resource = resources[next];
                }
                crossterm::event::KeyCode::Char('s')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    if state.username.is_empty() || state.password.is_empty() {
                        state.message = Some("Nothing to save".to_string());
                    } else {
                        state.message = match crate::credentials::save_credential_at(
                            &self.credentials_path,
                            state.resource,
                            &state.username,
                            &state.password,
                        ) {
                            Ok(()) => Some("Saved".to_string()),
                            Err(e) => Some(format!("Save failed: {}", e)),
                        };
                    }
                }
                crossterm::event::KeyCode::Tab => {
                    state.focus = match state.focus {
                        LoginField::Username => LoginField::Password,
                        LoginField::Password => LoginField::Username,
                    };
                }
                crossterm::event::KeyCode::Enter => {
                    if !state.username.is_empty() && !state.password.is_empty() {
                        let result = (
                            state.resource,
                            state.username.clone(),
                            state.password.clone(),
                        );
                        self.modal = Modal::None;
                        return Some(result);
                    }
                }
                crossterm::event::KeyCode::Char(c)
                    if !key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    match state.focus {
                        LoginField::Username => state.username.push(c),
                        LoginField::Password => state.password.push(c),
                    }
                }
                crossterm::event::KeyCode::Backspace => match state.focus {
                    LoginField::Username => {
                        state.username.pop();
                    }
                    LoginField::Password => {
                        state.password.pop();
                    }
                },
                _ => {}
            }
        }
        None
    }

    /// The login modal's own rendering: the resource tabs, the two fields, the saved-indicator
    /// and the hint row.
    pub fn render_login_modal(&self, frame: &mut Frame, area: Rect, config: &Config) {
        if let Modal::Login(ref state) = self.modal {
            let popup = centered_rect(50, 40, area);
            // ratatui's Buffer::set_style *patches* a cell's style (only
            // overwriting fields the new Style explicitly sets), it
            // doesn't replace the cell outright -- a plain background
            // fill here left every character already drawn by the main
            // view underneath fully intact (same glyph, same foreground
            // colour), which is exactly the "menu still shows the main
            // window's text/panel borders through it" bug report, and
            // also explains the unrelated-looking "areas turn an
            // unexpected grey" report: patched-in black backgrounds
            // behind *unpatched* foreground colours/glyphs don't read as
            // a clean fill. `Clear` actually resets each cell (glyph and
            // style) before the modal's own opaque block draws on top, so
            // the popup is genuinely self-contained; nothing outside its
            // bounds is touched at all.
            frame.render_widget(Clear, popup);

            let bg_color = self.theme.main_bg.to_color();
            let fg_color = self.theme.main_fg.to_color();

            let block = self
                .modal_block(self.theme.primary_color(), config)
                .title(Span::styled(
                    " Login ",
                    Style::default().fg(self.theme.primary_color()),
                ));

            let inner = block.inner(popup);
            frame.render_widget(block, popup);

            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(inner);

            // The resource tabs: one per id the credentials store knows,
            // the selected one highlighted. Left/Right switch them.
            let tabs: Vec<Span> = crate::credentials::LOGIN_RESOURCES
                .iter()
                .map(|&resource| {
                    let label = format!(" [{}] ", resource);
                    if resource == state.resource {
                        Span::styled(
                            label,
                            Style::default()
                                .fg(self.theme.primary_color())
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        Span::styled(
                            label,
                            Style::default().fg(self.theme.inactive_fg.to_color()),
                        )
                    }
                })
                .collect();
            frame.render_widget(Paragraph::new(Line::from(tabs)), rows[0]);

            // A field's border is its focus indicator: the primary
            // accent while the cursor is in it, the divider line
            // otherwise -- the same rule the zone frames follow.
            let user_style = if state.focus == LoginField::Username {
                Style::default()
                    .fg(self.theme.primary_color())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.theme.div_line.to_color())
            };

            let pass_style = if state.focus == LoginField::Password {
                Style::default()
                    .fg(self.theme.primary_color())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.theme.div_line.to_color())
            };

            let user_block = self
                .modal_block(user_style.fg.unwrap_or(fg_color), config)
                .title(Span::styled(
                    "Username",
                    Style::default().fg(self.theme.secondary_color()),
                ));
            frame.render_widget(
                Paragraph::new(state.username.as_str())
                    .style(Style::default().bg(bg_color).fg(fg_color))
                    .block(user_block),
                rows[1],
            );

            // The saved indicator: what the store already holds for the
            // selected resource, so the user knows before typing.
            let saved = match crate::credentials::load_credential_at(
                &self.credentials_path,
                state.resource,
            ) {
                Some((user, _)) => format!("Saved: user='{}'", user),
                None => "Not saved".to_string(),
            };
            frame.render_widget(
                Paragraph::new(Span::styled(
                    saved,
                    Style::default().fg(self.theme.inactive_fg.to_color()),
                )),
                rows[2],
            );

            let pass_display = if state.password.is_empty() {
                String::new()
            } else {
                "*".repeat(state.password.len())
            };

            let pass_block = self
                .modal_block(pass_style.fg.unwrap_or(fg_color), config)
                .title(Span::styled(
                    "Password",
                    Style::default().fg(self.theme.secondary_color()),
                ));
            frame.render_widget(
                Paragraph::new(pass_display.as_str())
                    .style(Style::default().bg(bg_color).fg(fg_color))
                    .block(pass_block),
                rows[3],
            );

            // Ctrl+S feedback, when there is any.
            if let Some(message) = state.message.as_deref() {
                frame.render_widget(
                    Paragraph::new(Span::styled(
                        message,
                        Style::default().fg(self.theme.secondary_color()),
                    )),
                    rows[4],
                );
            }

            frame.render_widget(
                Paragraph::new(Span::styled(
                    "[←/→] resource  [Tab] field  [Enter] login  [Ctrl+S] save  [Esc] cancel",
                    Style::default().fg(self.theme.inactive_fg.to_color()),
                )),
                rows[5],
            );
        }
    }
}
