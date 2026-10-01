//! The health check modal: what the
//! check reports and how the results are drawn. Extracted from
//! `ui/app.rs` together with the login modal; `render_modal` dispatches
//! to `render_health_modal`.

use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::config::Config;
use crate::torrserver::api::TorrServer;
use crate::ui::view::{centered_rect, App, Modal};

/// One line of the health check about the saved cookie file.
///
/// It used to be `Path::new("cookies.txt")` written into the check
/// itself: a path relative to whatever directory doris was started in,
/// naming a file nothing else in the app reads. With the cookie path now
/// configurable -- and `--cookie-file` able to point somewhere else
/// entirely -- a user with a real session elsewhere was told their file
/// was missing while a stray `cookies.txt` in the current directory was
/// the one being measured. The path is the one the app will actually
/// use, passed in.
pub fn cookie_file_status(path: &std::path::Path) -> String {
    if !path.exists() {
        return format!("\u{26a0} Cookie file: not found ({})", path.display());
    }
    match crate::sources::cookies::load_from_file(path) {
        Ok(c) if !c.is_empty() => {
            format!(
                "\u{2714} Cookie file: {} cookies ({})",
                c.len(),
                path.display()
            )
        }
        _ => format!("\u{26a0} Cookie file: empty/invalid ({})", path.display()),
    }
}

impl App {
    pub async fn health_check(&self, cookie_file: Option<&std::path::Path>) -> Vec<String> {
        let mut results = Vec::new();

        results.push("=== HEALTH CHECK ===".into());

        let browser_binary = match crate::browser::detect::detect_browser(None) {
            Ok((kind, path)) => {
                results.push(format!(
                    "{} Browser: {} [{}]",
                    "\u{2714}",
                    kind,
                    path.display()
                ));
                Some(path)
            }
            Err(e) => {
                results.push(format!("{} Browser: NOT FOUND ({})", "\u{2718}", e));
                None
            }
        };

        let has_xvfb = std::process::Command::new("which")
            .arg("Xvfb")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if has_xvfb {
            results.push(format!("{} Xvfb: available", "\u{2714}"));
        } else {
            results.push(format!(
                "{} Xvfb: not found (needed to run browser hidden)",
                "\u{2718}"
            ));
        }

        // "Available" means available *for the browser named above*:
        // the patched cache is one file per browser major now, and a
        // system driver only counts when its own version says so
        // (`cdp::driver_ready_for`).
        let has_chromedriver = browser_binary
            .as_ref()
            .map(|binary| crate::browser::cdp::driver_ready_for(binary))
            .unwrap_or(false);
        if has_chromedriver {
            results.push(format!("{} Chromedriver: patched/available", "\u{2714}"));
        } else {
            results.push(format!(
                "{} Chromedriver: will be downloaded on first run",
                "\u{26a0}"
            ));
        }

        let ts_url = self.torrserver_url.clone();
        // Was: tokio::runtime::Handle::current().block_on(...), which
        // panics with "Cannot start a runtime from within a runtime" --
        // health_check() always runs as part of the already-running
        // tokio runtime (it's called from the main event loop), so
        // block_on-ing that same runtime's handle is illegal. Making
        // this function itself async and.await-ing the request, like
        // every other network call in the app, is the fix.
        //
        // The probe itself is `TorrServer::is_reachable` -- the same call
        // `play` makes, with the same timeout. A second copy of it here
        // was a copy that could answer differently from the code it is
        // reporting on.
        let ts_reachable = TorrServer::new(&ts_url).is_reachable().await;
        if ts_reachable {
            results.push(format!("{} TorrServer: reachable ({})", "\u{2714}", ts_url));
        } else {
            results.push(format!(
                "{} TorrServer: NOT reachable ({})",
                "\u{2718}", ts_url
            ));
        }

        match crate::credentials::load_credentials() {
            Some((user, _)) => {
                results.push(format!("{} Saved credentials: user='{}'", "\u{2714}", user))
            }
            None => results.push(format!("{} Saved credentials: none", "\u{2718}")),
        }

        match cookie_file {
            Some(path) => results.push(cookie_file_status(path)),
            None => results.push(format!(
                "{} Cookie file: not saved (Options -> general)",
                "\u{26a0}"
            )),
        }

        let sources_line = crate::sources::source::KNOWN_SOURCES
            .iter()
            .map(|s| {
                if s.implemented {
                    format!("{}{}", "\u{2714} ", s.label)
                } else {
                    format!("{}{} (planned)", "\u{26a0} ", s.label)
                }
            })
            .collect::<Vec<_>>()
            .join("   ");
        results.push(format!("Sources: {}", sources_line));

        results.push("".into());
        results.push("Press Esc to close".into());
        results
    }

    /// The health check modal's own rendering: the results list,
    /// coloured by the mark each line carries. `&self` because it
    /// only reads the modal state and the theme.
    pub fn render_health_modal(&self, frame: &mut Frame, area: Rect, config: &Config) {
        if let Modal::HealthCheck(ref lines) = self.modal {
            let popup = centered_rect(70, 80, area);
            frame.render_widget(Clear, popup);

            let block = self
                .modal_block(self.theme.primary_color(), config)
                .title(Span::styled(
                    " Health Check ",
                    Style::default().fg(self.theme.primary_color()),
                ));

            let inner = block.inner(popup);
            frame.render_widget(block, popup);

            // The same severity mapping the detail log uses: a pass in
            // the secondary accent, a failure in the error accent, a
            // warning and a section header in the primary -- the marks
            // (✔/✘/⚠) say it in glyphs too, so colour is a second
            // channel and never the theme-blind one.
            let display_lines: Vec<Line> = lines
                .iter()
                .map(|l| {
                    if l.contains("\u{2714}") {
                        Line::from(Span::styled(
                            l.as_str(),
                            Style::default().fg(self.theme.secondary_color()),
                        ))
                    } else if l.contains("\u{2718}") {
                        Line::from(Span::styled(
                            l.as_str(),
                            Style::default().fg(self.theme.error_color()),
                        ))
                    } else if l.contains("\u{26a0}") {
                        Line::from(Span::styled(
                            l.as_str(),
                            Style::default().fg(self.theme.primary_color()),
                        ))
                    } else if l.starts_with("===") {
                        Line::from(Span::styled(
                            l.as_str(),
                            Style::default()
                                .fg(self.theme.primary_color())
                                .add_modifier(Modifier::BOLD),
                        ))
                    } else {
                        Line::from(Span::styled(
                            l.as_str(),
                            Style::default().fg(self.theme.main_fg.to_color()),
                        ))
                    }
                })
                .collect();

            // Same as the detail modal: `Clear` plus `modal_block`'s
            // `main_bg` is the popup's background, so the panel never
            // paints a colour the theme did not choose.
            let list = Paragraph::new(display_lines);
            frame.render_widget(list, inner);
        }
    }
}
