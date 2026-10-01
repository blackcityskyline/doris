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
        // `begin_search`, which decides it from whether this is a new
        // query or a re-ask of the one on screen.
        self.ui.begin_search(&query);
        // Rows now arrive one source at a time, so there is no
        // single moment where the old list gets replaced by the new one:
        // the table empties on the first answer to arrive, and each
        // source appends into it. The per-source records belong to the
        // old query and go with it.
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
        // now stale and gets dropped when it lands.
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
        // answer one are asked, and the merged list is ordered
        // freshest-first rather than by seeds.
        let browsing = query.trim().is_empty();
        self.ui.browsing = browsing;
        let selected = orchestrator::selected_sources(
            &self.config.enabled_sources,
            self.ui.active_group,
            browsing,
        );
        if selected.is_empty() {
            // Two ways to get here, and they have different fixes: no
            // source is checked at all, or nothing the panel reaches
            // serves the selected category -- the orchestrator words the
            // second one by what would actually change it.
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
            // spend a deferred row drop: spend it here or the table
            // keeps rows for a question that was refused outright.
            self.ui.take_pending_clear();
            return;
        }

        let plan =
            orchestrator::dispatch_plan(&selected, &self.source_offsets, &self.source_has_more);
        if plan.is_empty() {
            // Every selected source already reported its last page, so no
            // SourceDone is coming: close the generation here instead of
            // leaving the UI Searching -- and spend the deferred row
            // drop, which the answer that will never arrive would have.
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
            // a fresh hit needs no task at all -- it just has to
            // arrive like the normal answer would, so the offsets,
            // paging verdict and log line all update through the same
            // path. The category is part of the key: the same words
            // at the same offset under a different category are
            // different pages, so an "all" hit must never answer a
            // "Movies" request.
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
                    // spawned nothing else will ever speak for it: it
                    // still owes the user a line.
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
            // server-side will, one that cannot returns what it has --
            // and the view keeps only the rows claiming this category,
            // so an unhonoured category reads as fewer rows rather than
            // as a category nobody actually applied.
            req.category = self.ui.active_group;
            // Login walk first, then the search, in one task. The two
            // halves used to be two `tokio::spawn` calls around a
            // `cached_fetch`, differing only in what went inside the
            // `async move` -- so the arguments that have to agree (the
            // timeout, the channel, the cache) were written twice and
            // nothing made them agree.
            let task = {
                // If "Save cookies" is off, don't pass a cookie file
                // path through at all -- see do_login for the same
                // gating.
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
                tokio::spawn(orchestrator::run_source(
                    info.id,
                    generation,
                    orchestrator::cached_fetch(fetch, Arc::clone(&self.cache), key),
                    orchestrator::PER_SOURCE_TIMEOUT,
                    tx.clone(),
                ))
            };
            tasks.push((info.id, task));
        }

        // Always coordinate, even with an empty task list: with nothing
        // spawned there is nothing to wait for, and sending
        // SearchComplete through the same channel is what keeps it
        // *behind* any SourceDone events already queued from cache hits
        // (finishing here instead would close the generation before its
        // own rows arrived and read paging verdicts nobody had recorded
        // yet).
        tokio::spawn(orchestrator::coordinate(generation, tasks, tx));
    }

    pub(super) async fn load_more(&mut self, query: String) {
        self.ui.state = AppState::Searching;
        // Same generation as the results already on screen: the next page
        // appends to them instead of being treated as a superseded search.
        // Which sources get asked, and from which cursor, is decided per
        // source inside `dispatch_search`.
        let generation = self.search_generation;
        self.dispatch_search(query, generation).await;
    }
}
