use axum::{
    extract::Query,
    http::{HeaderMap, StatusCode},
    routing::get,
    Json, Router,
};
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Where a search may be asked for from.
///
/// The service listens on loopback and it *acts* -- a request starts a
/// search -- so answering every origin would let any page a browser
/// happens to be on drive this machine. An allow list is the difference
/// between "the sites the extension is installed for" and "the internet".
///
/// These are the extensions' own host permissions, read off the add-on's
/// manifest, so adding a site here and there is the same edit twice.
/// Subdomains are not matched: `www.imdb.com` and `imdb.com` are listed
/// separately rather than by suffix, because `evil-imdb.com` is not imdb.
/// Public because it is a contract with something outside this crate: the
/// add-on's `host_permissions` are the same list, and a test compares the two
/// so they cannot drift apart in silence.
pub const ALLOWED_ORIGINS: &[&str] = &[
    "https://www.imdb.com",
    "https://imdb.com",
    "https://trakt.tv",
    "https://www.trakt.tv",
    "https://kinopoisk.ru",
    "https://www.kinopoisk.ru",
    "https://lampa.mx",
    "https://www.lampa.mx",
    // Local origins, for a page served off the developer's own machine.
    "http://localhost",
    "http://127.0.0.1",
];

/// Whether an origin may be answered, ignoring scheme differences only in
/// so far as the list above already spells both out.
pub fn origin_allowed(origin: &str) -> bool {
    let origin = origin.trim().trim_end_matches('/');
    // An empty Origin is a caller on the machine: curl, the CLI, a
    // non-browser client. There is no page behind it that a browser could
    // have been tricked into sending anything.
    ALLOWED_ORIGINS.contains(&origin) || origin.is_empty()
}

/// Whether this caller may start a search.
///
/// Two questions live in this file and they must not be one question. *May
/// it act?* is about the origin being on the list, or there being no
/// origin at all -- curl and the CLI are on the machine, and there is no
/// page behind them that a browser could have been tricked into sending
/// anything. *What may it be told?* is narrower: only a caller that named
/// an origin may be given a CORS header echoing one.
///
/// Echoing rather than `*` is the point of the list. The service listens on
/// loopback and it *acts*, so a wildcard would let any page a browser
/// happens to be on drive this machine. And a missing header is not an
/// oversight the browser papers over: the request still arrives and the
/// search still runs, so an origin that is not allowed would still cost the
/// user a search while the caller sees an error. Which is why the refusal
/// is a real `403` rather than silence.
pub fn may_answer(origin: Option<&str>) -> bool {
    match origin {
        None => true,
        Some(origin) => origin_allowed(origin),
    }
}

/// The origin to echo back in `Access-Control-Allow-Origin`, or `None` for
/// a caller that gets no header: one that named none, or one not on the
/// list.
pub fn allowed_origin(origin: Option<&str>) -> Option<String> {
    let origin = origin.map(|o| o.trim().trim_end_matches('/'))?;
    origin_allowed(origin).then(|| origin.to_string())
}

/// The headers every answer carries. `false` means "nothing to add", which
/// is not a refusal -- see [`may_answer`].
pub fn cors_headers(origin: Option<&str>, headers: &mut HeaderMap) -> bool {
    let Some(allow) = allowed_origin(origin) else {
        return false;
    };
    headers.insert(
        "access-control-allow-origin",
        allow
            .parse()
            .expect("an allow list entry is a header value"),
    );
    headers.insert(
        "access-control-allow-methods",
        "GET, OPTIONS".parse().expect("static"),
    );
    // Without this a shared cache would serve one origin's allow header to
    // another, which is the wildcard problem wearing a different hat.
    headers.insert("vary", "Origin".parse().expect("static"));
    true
}

pub struct BridgeServer {
    tx: mpsc::UnboundedSender<String>,
    #[allow(dead_code)]
    port: u16,
}

impl BridgeServer {
    pub fn new(tx: mpsc::UnboundedSender<String>, port: u16) -> Self {
        Self { tx, port }
    }

    pub async fn start(&mut self) -> Result<(), anyhow::Error> {
        let tx = self.tx.clone();
        let app = Router::new()
            .route("/search", get(search_handler).options(preflight_handler))
            // "Are you there", and it does nothing. The add-on asks this
            // while it holds a title the user pressed with doris closed:
            // the bridge lives inside the app, so a title sent to a machine
            // with no app on it has nowhere to go, and the only way to find
            // out whether the app is back is to ask something that does not
            // start a search.
            .route("/ping", get(ping_handler).options(preflight_handler))
            .with_state(tx);

        let addr = format!("127.0.0.1:{}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                crate::log::log("bridge", &format!("bridge server stopped: {e}"));
            }
        });

        Ok(())
    }
}

/// The preflight a browser sends before the real request.
///
/// Answered without touching the search: a `OPTIONS` is the browser asking
/// whether it is allowed, and answering it by starting a search would
/// make the panel react to a question nobody asked.
async fn preflight_handler(
    headers: HeaderMap,
    axum::extract::State(tx): axum::extract::State<mpsc::UnboundedSender<String>>,
) -> (StatusCode, axum::http::HeaderMap) {
    let _ = tx;
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let mut out = axum::http::HeaderMap::new();
    cors_headers(origin.as_deref(), &mut out);
    let status = if may_answer(origin.as_deref()) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::FORBIDDEN
    };
    (status, out)
}

/// Answered without touching the search, and without a question in it.
///
/// The add-on polls this while it has something queued, so the cost of it
/// being free matters: it is asked every couple of seconds for as long as a
/// title is waiting, and a route that started a search would turn "the app
/// came back" into a search per poll.
async fn ping_handler(
    headers: HeaderMap,
    axum::extract::State(_tx): axum::extract::State<mpsc::UnboundedSender<String>>,
) -> (StatusCode, axum::http::HeaderMap, Json<serde_json::Value>) {
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let mut out = axum::http::HeaderMap::new();
    cors_headers(origin.as_deref(), &mut out);
    (
        StatusCode::OK,
        out,
        Json(serde_json::json!({"success": true, "doris": true})),
    )
}

async fn search_handler(
    Query(params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    axum::extract::State(tx): axum::extract::State<mpsc::UnboundedSender<String>>,
) -> (StatusCode, axum::http::HeaderMap, Json<serde_json::Value>) {
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    // Refused outright, not answered without CORS. Starting a search for an
    // origin we do not recognise is the thing worth preventing, and a
    // browser will not show the caller an error either way.
    if !may_answer(origin.as_deref()) {
        return (
            StatusCode::FORBIDDEN,
            axum::http::HeaderMap::new(),
            Json(serde_json::json!({
                "success": false,
                "error": "origin not allowed",
            })),
        );
    }

    let query = params.get("q").cloned().unwrap_or_default();
    let _ = tx.send(query.clone());

    let mut out = axum::http::HeaderMap::new();
    cors_headers(origin.as_deref(), &mut out);
    (
        StatusCode::OK,
        out,
        Json(serde_json::json!({"success": true, "query": query})),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_the_extensions_own_permissions() {
        for origin in [
            "https://www.imdb.com",
            "https://trakt.tv",
            "https://kinopoisk.ru",
            "https://lampa.mx",
        ] {
            assert!(
                origin_allowed(origin),
                "{origin} is in the add-on's manifest"
            );
        }
    }

    #[test]
    fn a_trailing_slash_is_the_same_origin() {
        assert!(origin_allowed("https://www.imdb.com/"));
        assert!(origin_allowed(" https://trakt.tv "));
    }

    #[test]
    fn a_lookalike_host_is_not_the_host() {
        for origin in [
            "https://evil-imdb.com",
            "https://imdb.com.evil.net",
            "https://notimdb.com",
        ] {
            assert!(!origin_allowed(origin), "{origin} must not be answered");
        }
    }

    #[test]
    fn no_origin_at_all_is_a_local_caller_and_is_answered() {
        // curl and the CLI have no Origin, and they are on the machine.
        assert!(may_answer(None), "a caller with no Origin is local");
        assert!(may_answer(Some("")), "and an empty Origin is the same case");
        // Answered, but with no header: there is no origin to allow, and
        // `*` is not how you say "nothing".
        assert!(
            allowed_origin(None).is_none(),
            "nothing is echoed to a caller that named no origin"
        );
    }

    #[test]
    fn a_refused_origin_gets_no_allow_header_at_all() {
        let mut headers = HeaderMap::new();
        cors_headers(Some("https://evil.example"), &mut headers);
        assert!(
            headers.get("access-control-allow-origin").is_none(),
            "a wildcard or an echoed unknown origin would let any page drive this machine"
        );
    }

    #[test]
    fn an_allowed_origin_is_echoed_back_not_wildcarded() {
        let mut headers = HeaderMap::new();
        cors_headers(Some("https://www.imdb.com"), &mut headers);
        assert_eq!(
            headers.get("access-control-allow-origin").unwrap(),
            "https://www.imdb.com"
        );
        assert_eq!(headers.get("vary").unwrap(), "Origin", "cached per origin");
    }
}
