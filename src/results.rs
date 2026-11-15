use std::collections::HashMap;

use crate::config::Config;
use crate::sources::models::TorrentItem;
use crate::sources::ordering::{dedupe_by_hash, order};
use crate::ui::view::{App as UiApp, AppState};

/// The single log line describing how one source's dispatch ended: `rutor: 42 results` /
/// `rutracker: HTTP 503`.
pub fn source_outcome_line(source: &str, outcome: &Result<usize, String>) -> String {
    match outcome {
        Ok(count) => format!("{}: {} results", source, count),
        Err(err) => format!("{}: {}", source, err),
    }
}
/// Merge one source's page into the Results panel and log its B0.3 outcome line -- unless the
/// event came from a dispatch that a newer `start_search` has since superseded, in which case
/// it is dropped and `false` is returned (B0.2: a late answer from the previous query used to
/// land after the new one started and overwrite the fresh one's results).
pub fn apply_source_done(
    ui: &mut UiApp,
    event_generation: u64,
    current_generation: u64,
    source: &str,
    items: Vec<crate::sources::models::TorrentItem>,
    error: Option<&str>,
) -> bool {
    if event_generation != current_generation {
        ui.add_log("Dropped stale search results (superseded by a newer search)");
        return false;
    }
    let outcome: Result<usize, String> = match error {
        None => Ok(items.len()),
        Some(err) => Err(err.to_string()),
    };
    ui.add_log(&source_outcome_line(source, &outcome));
    // The first answer of a re-ask is what retires the old rows: they
    ui.take_pending_clear();
    ui.results.extend(items);
    ui.update_filter();
    true
}
/// Every source of `generation` reported in (or failed to): nothing more is coming for it, so
/// the UI goes idle.
pub fn finish_search(
    ui: &mut UiApp,
    event_generation: u64,
    current_generation: u64,
    has_more: &HashMap<String, bool>,
) -> bool {
    if event_generation != current_generation {
        ui.add_log("Dropped stale search completion (superseded by a newer search)");
        return false;
    }
    present_results(ui);
    ui.all_loaded = !has_more.values().any(|&more| more);
    ui.state = AppState::Idle;
    true
}

/// Dedupe the merged multi-source list and put it into its default order This runs exactly once
/// per generation -- when every source has answered -- because reordering while sources are
/// still arriving would move rows out from under the user's selection.
pub fn present_results(ui: &mut UiApp) {
    let before = ui.results.len();
    let ordered = order(&dedupe_by_hash(&ui.results), ui.sort, ui.browsing);
    let removed = before - ordered.len();
    install_order(ui, ordered);
    if removed > 0 {
        ui.add_log(&format!("Removed {} duplicate results", removed));
    }
}

/// Re-order the rows already on screen, for the `o` key that changed
/// `sort` and nothing else. Same path as [`present_results`] minus the
/// dedupe: the list has been deduped once already, and running it again
/// would log a second "Removed 0 duplicates" line nobody asked for.
pub fn resort_results(ui: &mut UiApp) {
    let ordered = order(&ui.results, ui.sort, ui.browsing);
    install_order(ui, ordered);
}

/// Put `ordered` in place of the current rows and keep the cursor on the row
/// the user was reading, wherever it ended up.
///
/// One function, because both callers need exactly this and the anchor rule
/// is the part that is easy to get wrong twice.
fn install_order(ui: &mut UiApp, ordered: Vec<TorrentItem>) {
    let anchor = ui.results.get(ui.selected).cloned();

    ui.results = ordered;
    // Reordering moves rows under every saved index, the filter anchor
    ui.filter_anchor = None;

    if let Some(anchor) = anchor {
        let keep = ui
            .results
            .iter()
            .position(|row| row.page_url == anchor.page_url && row.title == anchor.title);
        ui.selected = keep.unwrap_or_else(|| ui.selected.min(ui.results.len().saturating_sub(1)));
    } else {
        ui.selected = 0;
    }
    ui.update_filter();
}

/// Resolve the cookie file path used for Rutracker login, or `None` if "Save cookies" is off.
pub fn resolve_cookie_file(
    config: &Config,
    cli_override: Option<&std::path::Path>,
) -> Option<std::path::PathBuf> {
    if !config.save_cookies {
        return None;
    }
    cli_override
        .map(|p| p.to_path_buf())
        .or_else(|| Some(std::path::PathBuf::from(&config.cookie_file)))
}
