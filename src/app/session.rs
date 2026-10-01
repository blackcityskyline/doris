//! Owning the browser and the live sources behind it.
//!
//! Both are built once and reused for the session, so the browser behind
//! a source -- and the session it holds -- is not rebuilt per query.

use super::*;

impl App {
    /// `home_url` comes from the registry entry of the source asking for
    /// the browser: the `Source` instance can't supply it, because
    /// building that instance is exactly what needs the browser.
    pub(super) async fn get_browser(&mut self, home_url: &str) -> Result<Arc<Mutex<Browser>>> {
        if let Some(ref b) = self.browser {
            return Ok(Arc::clone(b));
        }

        let browser_choice = self
            .args
            .browser
            .as_deref()
            .or(self.config.browser.as_deref());
        let browser_priority = detect::parse_priority(&self.config.browser_priority);
        let (kind, path) = detect::detect_browser_with_priority(browser_choice, &browser_priority)?;
        self.ui.add_log(&format!(
            "Launching {} ({})...",
            kind, self.browser_visibility
        ));

        // What to keep the browser away from is a fact about the site that
        let block_hosts: &'static [&'static str] = crate::sources::source::KNOWN_SOURCES
            .iter()
            .find(|s| s.home_url == home_url)
            .map(|s| s.block_hosts)
            .unwrap_or(&[]);

        let browser = Browser::launch(
            &path,
            self.browser_visibility,
            home_url,
            self.config.close_browser_on_exit,
            block_hosts,
        )
        .await?;
        let browser = Arc::new(Mutex::new(browser));
        self.browser = Some(Arc::clone(&browser));

        Ok(browser)
    }

    /// Build (once) and reuse the live `Source` for `id` -- the one path from `app.rs` onto a
    /// concrete source type, via the registry and `source::build_source`.
    pub(super) async fn get_source(&mut self, id: &str) -> Result<Arc<dyn Source>> {
        if let Some(existing) = self.sources.get(id) {
            return Ok(Arc::clone(existing));
        }
        let info =
            source::get_source(id).ok_or_else(|| anyhow::anyhow!("unknown source '{}'", id))?;
        let browser = if info.requires_browser {
            Some(self.get_browser(info.home_url).await?)
        } else {
            None
        };
        let built = source::build_source(info.id, SourceEnv { browser })?;
        self.sources.insert(info.id, Arc::clone(&built));
        Ok(built)
    }

    /// The instance a result row is played through: plain-HTTP sources
    /// are built on the spot, browser-backed ones must already be in the
    /// cache -- streaming never launches a browser itself, it
    /// reports "No browser session - search first" instead.
    pub(super) async fn source_for_row(&mut self, id: &'static str) -> Result<Arc<dyn Source>> {
        if source_needs_browser(id) {
            self.sources
                .get(id)
                .map(Arc::clone)
                .ok_or_else(|| anyhow::anyhow!("No browser session - search first"))
        } else {
            self.get_source(id).await
        }
    }

    /// Log in with the credentials the modal collected, for the resource its tab had selected.
    pub(super) async fn do_login(&mut self, resource: &str, username: &str, password: &str) {
        self.ui.add_log(&format!("Logging in as '{}'...", username));

        if self.config.save_credentials {
            let _ = crate::credentials::save_credential_at(
                &self.ui.credentials_path,
                resource,
                username,
                password,
            );
        }

        let source = match self.get_source("rutracker").await {
            Ok(s) => s,
            Err(e) => {
                self.ui.add_log(&format!("Browser error: {}", e));
                return;
            }
        };

        let auth = AuthContext {
            cookie_file: self.resolve_cookie_file(),
            username: Some(username.to_string()),
            password: Some(password.to_string()),
        };
        let event_tx_login = self.event_handler.sender();
        let event_tx_result = self.event_handler.sender();
        let log: LogFn = Arc::new(move |msg: &str| {
            let _ = event_tx_login.send(Event::StreamLog(msg.to_string()));
        });

        tokio::spawn(async move {
            match source.ensure_logged_in(&auth, &log).await {
                Ok(true) => {
                    let _ = event_tx_result.send(Event::LoginResult(true));
                }
                Ok(false) => {
                    let _ = event_tx_result.send(Event::LoginResult(false));
                }
                Err(e) => {
                    log(&format!("LOGIN ERROR: {}", e));
                    let _ = event_tx_result.send(Event::LoginResult(false));
                }
            }
        });
    }
}
