//! Asking every checked source at once and folding the answers back in.
//!
//! A generation number goes out with each dispatch and comes back with
//! its rows: an answer from a previous query is dropped rather than
//! landing on top of the new one.

use super::*;

impl App {
    /// A category switch asks again rather than re-labelling what is already on screen.
    pub(super) async fn reask_for_category(&mut self) {
        if let Some(query) = self.ui.search_query.clone() {
            self.start_search(query).await;
        }
    }

    pub(super) async fn start_search(&mut self, query: String) {
        // The row drop (and the flags Enter reads) belong to
        self.ui.begin_search(&query);
        // Rows now arrive one source at a time, so there is no
        self.ui.source_status.clear();
        self.source_has_more.clear();
        self.source_offsets.clear();
        let category = match self.ui.active_group {
            Some(group) => format!(" [{}]", group.label()),
            None => String::new(),
        };
        self.ui.add_log(&format!(
            "Searching '{}'{} across {}...",
            query,
            category,
            sources_summary(&self.config)
        ));
        // New generation: anything still in flight for a previous query is
        self.search_generation += 1;
        let generation = self.search_generation;
        self.dispatch_search(query, generation).await;
    }

    /// Kick off the search for `query`: one task per source `orchestrator::selected_sources`
    /// picks (the Trackers panel's checkboxes, narrowed by the selected category) and
    /// `orchestrator::dispatch_plan` says is worth asking (a fresh search asks everyone, a
    /// "load more" asks only the sources that reported another page, each at its own cursor),
    /// each task under the per-source deadline.
    pub(super) async fn dispatch_search(&mut self, query: String, generation: u64) {
        // An empty query is browse mode: only sources that can
        let browsing = query.trim().is_empty();
        self.ui.browsing = browsing;
        let selected = orchestrator::selected_sources(
            &self.config.enabled_sources,
            self.ui.active_group,
            browsing,
        );
        if selected.is_empty() {
            // Two ways to get here, and they have different fixes: no
            let reason = match self.ui.active_group {
                Some(group) => {
                    orchestrator::nothing_to_ask_reason(&self.config.enabled_sources, group)
                }
                None => {
                    "No source is checked -- the Trackers panel (3) is where they are switched on."
                        .to_string()
                }
            };
            self.ui.add_log(&reason);
            self.ui.state = AppState::Idle;
            // No source was even asked, so no answer will arrive to
            self.ui.take_pending_clear();
            return;
        }

        let plan =
            orchestrator::dispatch_plan(&selected, &self.source_offsets, &self.source_has_more);
        if plan.is_empty() {
            // Every selected source already reported its last page, so no
            self.ui.take_pending_clear();
            finish_search(
                &mut self.ui,
                generation,
                self.search_generation,
                &self.source_has_more,
            );
            return;
        }

        let tx = self.event_handler.sender();
        let mut tasks: Vec<(&'static str, tokio::task::JoinHandle<()>)> = Vec::new();

        for (info, offset) in plan {
            // Cache lookup before anything else, browser launch included
            let key = CacheKey::new(
                info.id,
                &query,
                self.ui.active_group.map(source::Group::label),
                offset,
            );
            if let Some(done) = orchestrator::cached_source_done(&self.cache, &key, generation) {
                let _ = tx.send(done);
                continue;
            }

            let source = match self.get_source(info.id).await {
                Ok(s) => s,
                Err(e) => {
                    // This source can't run at all, and with no task
                    self.ui
                        .add_log(&source_outcome_line(info.id, &Err(e.to_string())));
                    self.ui
                        .source_status
                        .insert(info.id.to_string(), SourceStatus::Error(e.to_string()));
                    continue;
                }
            };

            self.ui
                .source_status
                .insert(info.id.to_string(), SourceStatus::Pending);
            let mut req = SearchRequest::new(query.clone(), offset);
            // The selection rides along: a source that can filter
            req.category = self.ui.active_group;
            // Login walk first, then the search, in one task. The two
            let task = {
                // If "Save cookies" is off, don't pass a cookie file
                let cookie_file = self.resolve_cookie_file();
                let username = self.args.username.clone();
                let password = self.args.password.clone();
                let saved_creds = crate::credentials::load_credentials();
                let event_tx_log = self.event_handler.sender();
                let fetch = async move {
                    if info.requires_browser {
                        let log: LogFn = Arc::new(move |msg: &str| {
                            let _ = event_tx_log.send(Event::StreamLog(msg.to_string()));
                        });
                        let (cred_user, cred_pass) = match (username, password) {
                            (Some(u), Some(p)) => (Some(u), Some(p)),
                            _ => match saved_creds {
                                Some((u, p)) => {
                                    log("Using saved credentials");
                                    (Some(u), Some(p))
                                }
                                None => (None, None),
                            },
                        };
                        let auth = AuthContext {
                            cookie_file,
                            username: cred_user,
                            password: cred_pass,
                        };
                        match source.ensure_logged_in(&auth, &log).await {
                            Ok(true) => log("SEARCH: logged in, proceeding with search"),
                            Ok(false) => log("SEARCH: not logged in, proceeding anyway"),
                            Err(e) => log(&format!("SEARCH: login error: {}", e)),
                        }
                    }
                    source.search(&req).await
                };
                // The deadline has to cover the login walk as well as the
                // page fetch, because both are inside `fetch`. A cold
                // login -- browser launch, Cloudflare, fill, submit --
                // took 26 s in a live run against a 25 s deadline, so the
                // login landed, cookies were written, and the deadline
                // then fired before the search ever started. That read
                // in the Trackers panel as "rutracker: timeout" with a
                // green LOGIN SUCCESSFUL in the log.
                //
                // The extra is only for sources that need a browser;
                // the rest still get the plain deadline.
                let budget = orchestrator::deadline_for(info.requires_browser);
                tokio::spawn(orchestrator::run_source(
                    info.id,
                    generation,
                    orchestrator::cached_fetch(fetch, Arc::clone(&self.cache), key),
                    budget,
                    tx.clone(),
                ))
            };
            tasks.push((info.id, task));
        }

        // Always coordinate, even with an empty task list: with nothing
        tokio::spawn(orchestrator::coordinate(generation, tasks, tx));
    }

    pub(super) async fn load_more(&mut self, query: String) {
        self.ui.state = AppState::Searching;
        // Same generation as the results already on screen: the next page
        let generation = self.search_generation;
        self.dispatch_search(query, generation).await;
    }
}
