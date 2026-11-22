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
        let tx = self.event_handler.sender();
        let log: LogFn = Arc::new(move |msg: &str| {
            let _ = tx.send(Event::StreamLog(msg.to_string()));
        });
        let args = self.args.clone();
        let config = self.config.clone();
        let visibility = self.browser_visibility;
        browser_for(&self.browser, &args, &config, visibility, home_url, &log).await
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
            if let Some(source) = self.sources.get(id) {
                return Ok(Arc::clone(source));
            }
            // A search builds a browser-backed source inside its own task, so
            // it need not be in the cache. With no browser up either there is
            // no session to act on, and saying so is better than a silent
            // launch from under a running stream.
            if self.browser.get().is_none() {
                anyhow::bail!("No browser session - search first");
            }
        }
        self.get_source(id).await
    }

    /// Open the browser-backed session a row needs, for a command that has no
    /// search behind it.
    ///
    /// In the TUI the search is what launches the browser, so a row on the
    /// table always has its session by the time Enter reaches it. `doris play
    /// --row '<row>'` is a single process that never searched, and it answered
    /// "No browser session - search first" to a row the caller was looking at
    /// -- advice it could not take, because it *is* the command. So the
    /// command does the walk the search would have done.
    pub(super) async fn open_source_session(&mut self, id: &'static str) -> Result<()> {
        let source = self.get_source(id).await?;
        if !source_needs_browser(id) {
            return Ok(());
        }
        let (username, password) = match (self.args.username.clone(), self.args.password.clone()) {
            (Some(u), Some(p)) => (Some(u), Some(p)),
            _ => match crate::credentials::load_credential(id) {
                Some((u, p)) => (Some(u), Some(p)),
                None => (None, None),
            },
        };
        let auth = AuthContext {
            cookie_file: self.resolve_cookie_file(),
            username,
            password,
        };
        let event_tx_log = self.event_handler.sender();
        let log: LogFn = Arc::new(move |msg: &str| {
            // The log file as well as the channel: a CLI has no Log zone to
            // read, so a failed walk must leave something behind.
            crate::log::log("auth", msg);
            let _ = event_tx_log.send(Event::StreamLog(msg.to_string()));
        });
        let _ = source.ensure_logged_in(&auth, &log).await?;
        Ok(())
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

        // The tab the credentials were typed into names the source: two
        // trackers, two accounts, one store.
        let source = match self.get_source(resource).await {
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
            // The Log zone only lives as long as the TUI; a login that failed
            // twice in a row is something to read about afterwards.
            crate::log::log("auth", msg);
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

/// The session's one browser, launched by whichever source needs it first.
///
/// It lives in a cell rather than in a plain field because a browser-backed
/// source is built inside its own search task: coming up takes seconds
/// (Xvfb, Chromedriver, Chrome), and the trackers that need no browser must
/// not be held back by it. A cell means the second search, and a command
/// acting on a row, find the same session instead of launching another.
pub(super) async fn browser_for(
    cell: &tokio::sync::OnceCell<Arc<Mutex<Browser>>>,
    args: &Args,
    config: &Config,
    visibility: BrowserVisibility,
    home_url: &str,
    log: &LogFn,
) -> Result<Arc<Mutex<Browser>>> {
    let launched = cell
        .get_or_try_init(|| launch_browser(args, config, visibility, home_url, log))
        .await?;
    Ok(Arc::clone(launched))
}

async fn launch_browser(
    args: &Args,
    config: &Config,
    visibility: BrowserVisibility,
    home_url: &str,
    log: &LogFn,
) -> Result<Arc<Mutex<Browser>>> {
    let browser_choice = args.browser.as_deref().or(config.browser.as_deref());
    let browser_priority = detect::parse_priority(&config.browser_priority);
    let (kind, path) = detect::detect_browser_with_priority(browser_choice, &browser_priority)?;
    log(&format!("Launching {} ({})...", kind, visibility));

    // What to keep the browser away from is a fact about the site that
    let block_hosts: &'static [&'static str] = crate::sources::source::KNOWN_SOURCES
        .iter()
        .find(|s| s.home_url == home_url)
        .map(|s| s.block_hosts)
        .unwrap_or(&[]);

    let browser = Browser::launch(
        &path,
        visibility,
        home_url,
        config.close_browser_on_exit,
        block_hosts,
    )
    .await?;
    Ok(Arc::new(Mutex::new(browser)))
}
