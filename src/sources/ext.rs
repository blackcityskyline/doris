//! ext.to, read out of the browser doris already has open -- the only tracker
//! here whose answer a plain HTTP client cannot get.
//!
//! Everything below was measured on 05.10.2026, and each of it cost a probe.
//!
//! **Cloudflare decides whether this source works at all.** The root `/` answers
//! a hidden browser in about ten seconds. `/browse/?q=` did **not**: five
//! attempts, waits of up to 120 seconds, three ways of navigating there (the
//! driver's `navigate`, `location.href`, the site's own search form), and the
//! challenge never cleared. With a *visible* browser on the user's own profile
//! the same URL showed a checkbox -- "Verify you are human" -- that one click
//! clears, and from then on the `cf_clearance` cookie answers for hours (the
//! next run reached the results in zero seconds). So ext needs the browser on
//! the native profile: a hidden one gets a fresh temporary profile per run and
//! starts from nothing every time. That is the same reason the sources that
//! work through Cloudflare say so in their docs.
//!
//! **Search.** `?q=` is the site's own spelling -- `/search/?searchstr=` is
//! gone and redirects to `/advanced/`, which is a form and not a search. The
//! categories are `?cat=1..8` (Movies, TV, Music, Games, Apps, Books, Anime,
//! Other) and a page holds 50 rows, paged with `&page=N`.
//!
//! **Nothing here is playable without an account.** An anonymous page carries no
//! magnet at all: the button is `javascript:void(0)` with a `data-id`, and the
//! ajax answers `{"success":false,"error":"Invalid session"}` -- because
//! `window.searchPageToken` and `<meta name="csrf-token">` are only filled in
//! for a session. Logged in, the same POST answers
//! `{"success":true,"url":"magnet:?xt=urn:btih:..."}`. So a row leaves here with
//! no magnet of its own, and doris reads it when the row is picked, the way
//! 1337x does -- one page load per row played, not fifty per search.
//!
//! **Login** is a modal on every page (`#auth-form`, fields `name` and
//! `password`), not a page: `https://ext.to/login/` answers "Page not found".

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::browser::cdp::Browser;
use crate::sources::cookies::{self, Cookie};

use super::format::format_date;
use super::models::TorrentItem;
use super::source::{Group, SearchPage, SearchRequest, Source};

pub const HOME_URL: &str = "https://ext.to/";
const SITE: &str = "https://ext.to";

/// Rows one browse page holds, live on `dune` (50, and the pager counts 50 per
/// page from there).
pub const PAGE_SIZE: usize = 50;

/// The four of ext's eight categories that map onto a [`Group`], read off the
/// browse sidebar live: `cat=1` Movies, `cat=2` TV, `cat=4` Games, `cat=7`
/// Anime. Music, Apps, Books and Other have no group in doris and are left
/// unfilterable rather than folded into a neighbouring one.
pub const CATEGORY_IDS: [(Group, i64); 4] = [
    (Group::Movies, 1),
    (Group::TV, 2),
    (Group::Games, 4),
    (Group::Anime, 7),
];

/// The groups this source can attribute a row to. One list for the live instance
/// and the registry entry, because the two answering the same question in two
/// places is how a source ends up offering a category its own rows never carry.
pub const EXT_GROUPS: &[Group] = &[Group::Movies, Group::TV, Group::Games, Group::Anime];

/// The same four, read back off a row's own category link -- a row can arrive
/// from a page asked without a category, so the group comes from the row rather
/// than from the request.
fn group_of_label(label: &str) -> Option<Group> {
    match label.trim().to_ascii_lowercase().as_str() {
        "movies" => Some(Group::Movies),
        "tv" => Some(Group::TV),
        "games" => Some(Group::Games),
        "anime" => Some(Group::Anime),
        _ => None,
    }
}

/// `/browse/?q=dune&cat=1&page=2` -- the site's own three slots, and nothing
/// more: it has no per-forum ids, no sort id worth pinning, and `page` only when
/// the offset is past the first page.
pub fn search_url(query: &str, offset: usize, category: Option<Group>) -> String {
    let query = query.trim();
    let page = offset / PAGE_SIZE + 1;
    let mut url = format!("{SITE}/browse/?q={}", urlencoding::encode(query));
    if let Some((_, id)) = CATEGORY_IDS
        .iter()
        .find(|(group, _)| Some(*group) == category)
    {
        url.push_str(&format!("&cat={id}"));
    }
    if page > 1 {
        url.push_str(&format!("&page={page}"));
    }
    url
}

/// The topic id out of a row's own page URL: ext spells a topic `/<slug>-<id>/`,
/// and the id is the number the magnet ajax wants.
pub fn topic_id(page_url: &str) -> Option<u64> {
    let last = page_url.trim_end_matches('/').rsplit('/').next()?;
    let digits = last.rsplit('-').next()?;
    if digits.len() < 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// One row, as the page hands it over (see [`ROWS_SCRIPT`]).
#[derive(Debug, Deserialize)]
struct ExtRow {
    title: String,
    href: String,
    #[serde(default)]
    size: String,
    #[serde(default)]
    seeds: String,
    #[serde(default)]
    leechers: String,
    /// The age cell's `title`, which is the real date -- the cell text is
    /// "2 years ago", which says nothing a date column can print.
    #[serde(default)]
    age_title: String,
    #[serde(default)]
    category: String,
}

/// The rows off one page, as [`ROWS_SCRIPT`] hands them over. Pure, so the
/// shape of the page can be checked without a browser: the numbers come off
/// labelled cells, and a label the site renames shows up here as a row with an
/// empty size rather than as a crash.
pub fn parse_rows(body: &str) -> Result<Vec<TorrentItem>> {
    let rows: Vec<ExtRow> =
        serde_json::from_str(body).context("ext: cannot read the rows the page gave")?;
    Ok(rows.into_iter().map(to_item).collect())
}

/// A row turned into a result: no magnet, no `.torrent`, and the topic URL as
/// `page_url`, which is what `fill_missing_magnet` later hands to
/// [`Source::resolve_magnet`].
fn to_item(row: ExtRow) -> TorrentItem {
    let mut item = TorrentItem {
        title: row.title,
        size: row.size,
        seeds: row.seeds.clone(),
        source: "ext".to_string(),
        page_url: format!("{SITE}{}", row.href),
        group: group_of_label(&row.category),
        leechers: row.leechers.trim().parse().unwrap_or(0),
        ..Default::default()
    };
    item.date = parse_ext_date(&row.age_title);
    item.fill_from_display();
    item
}

/// `06 April 2024` -> unix seconds. Empty when the cell had no `title`.
fn parse_ext_date(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        return String::new();
    }
    match chrono::NaiveDate::parse_from_str(title, "%d %B %Y") {
        Ok(date) => match date.and_hms_opt(0, 0, 0) {
            Some(midnight) => format_date(midnight.and_utc().timestamp()),
            None => String::new(),
        },
        Err(_) => String::new(),
    }
}

/// The rows, as the page's own DOM spells them. Every number is a labelled
/// `add-block-wrapper` (`Size`, `Seeds`, `Leechs`, `Age`), which is the one
/// thing on the page that is stable across the desktop and mobile markup: the
/// same values appear again in a `mobile-info-block`, and the desktop table
/// cells disappear on a narrow window.
const ROWS_SCRIPT: &str = r#"
async function scrape() {
  const rows = [];
  for (const row of document.querySelectorAll('tbody tr')) {
    const link = row.querySelector('a.torrent-title-link');
    if (!link) continue;
    const cells = {};
    for (const wrap of row.querySelectorAll('.add-block-wrapper')) {
      const label = wrap.querySelector('.add-block');
      const value = wrap.querySelector('span:not(.add-block)');
      if (label && value) cells[label.textContent.trim()] = value.textContent.trim();
    }
    const age = Array.from(row.querySelectorAll('.add-block-wrapper'))
      .find(w => (w.querySelector('.add-block') || {}).textContent === 'Age');
    const ageTitle = age ? ((age.querySelector('span[title]') || {}).title || '') : '';
    // The category is the first path link of the "Posted by X in <category> -
    // <subcategory>" line. The uploader link is spelled two ways on the same
    // page -- `/user/<nick>/` on some rows, `?user_nick=<nick>&with_adult=1` on
    // others (live, in the same result list) -- so "not /user/" alone is not
    // enough to skip it, and a filter that misses it reads the *uploader* as the
    // category and leaves every such row unattributed. A path that says nothing
    // about a user is the one that is the category.
    const category = Array.from(row.querySelectorAll('.related-posted a[href^="/"]'))
      .map(a => [a.getAttribute('href'), a.textContent.trim()])
      .find(([href]) => !href.includes('user'));
    rows.push({
      title: link.textContent.trim(),
      href: link.getAttribute('href'),
      size: cells['Size'] || '',
      seeds: cells['Seeds'] || '',
      leechers: cells['Leechs'] || '',
      age_title: ageTitle,
      category: category ? category[1] : ''
    });
  }
  return JSON.stringify(rows);
}
return await scrape();
"#;

/// One magnet or `.torrent` URL for a topic, read with the page's own tokens:
/// `window.pageToken` (or the search page's `window.searchPageToken`) is the
/// HMAC key and `<meta name="csrf-token">` the session. The signature is
/// `sha256("<id>|<unix seconds>|<token>")`, which is why this runs in the page
/// and not in Rust -- the browser already has `crypto.subtle`.
const MAGNET_SCRIPT: &str = r#"
async function magnet() {
  const meta = document.querySelector('meta[name="csrf-token"]');
  const token = window.pageToken || window.searchPageToken;
  const out = { token: !!token, csrf: !!meta, answers: [] };
  if (!token || !meta) return JSON.stringify(out);
  const id = '__ID__';
  const ts = Math.floor(Date.now() / 1000);
  const buf = await crypto.subtle.digest('SHA-256',
    new TextEncoder().encode(id + '|' + ts + '|' + token));
  const hmac = Array.from(new Uint8Array(buf)).map(b => b.toString(16).padStart(2, '0')).join('');
  const body = new URLSearchParams({
    torrent_id: id, download_type: 'magnet',
    timestamp: String(ts), hmac: hmac, sessid: meta.content
  });
  // The topic page's own button asks getTorrentMagnet.php; the search page's
  // asks getSearchMagnet.php. Both are tried because the right one depends on
  // which page we are standing on, and the wrong one answers 200 with
  // "Invalid request. Please refresh the page." -- measured, not guessed.
  for (const endpoint of ['/ajax/getTorrentMagnet.php', '/ajax/getSearchMagnet.php']) {
    try {
      const r = await fetch(endpoint, { method: 'POST', credentials: 'include',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: body.toString() });
      // The whole text, uncut: a magnet with its trackers runs to a few hundred
      // characters, and a cut answer is a JSON document that stops mid-string --
      // which reads as "the site said nothing", not as "we cut it off".
      out.answers.push({ endpoint: endpoint, status: r.status, text: await r.text() });
    } catch (e) {
      out.answers.push({ endpoint: endpoint, error: String(e) });
    }
  }
  return JSON.stringify(out);
}
return await magnet();
"#;

pub struct ExtSearcher {
    browser: Arc<Mutex<Browser>>,
    /// Interior mutability because `Source::ensure_logged_in` takes `&self`:
    /// an `Arc<dyn Source>` cannot hand out `&mut`. A memo, not a lock -- the
    /// browser mutex already serialises the work.
    logged_in: AtomicBool,
}

impl ExtSearcher {
    pub fn new(browser: Arc<Mutex<Browser>>) -> Self {
        Self {
            browser,
            logged_in: AtomicBool::new(false),
        }
    }

    /// Park the tab on `about:blank` once the answer is in hand: one ext results
    /// page is over a megabyte of markup with scripts still running.
    async fn park(&self) {
        let browser = self.browser.lock().await;
        if let Err(e) = browser.park().await {
            crate::log::log("search", &format!("idle park failed: {}", e));
        }
    }

    /// Cloudflare's page is `Just a moment...` until it is not, and on ext it
    /// can be waiting for a person to tick a box -- so this waits longer than
    /// rutracker's does, and says so in the log rather than returning a page of
    /// "verifying you are not a bot" to be parsed as zero results.
    async fn wait_cloudflare(browser: &Browser, log: &crate::sources::source::LogFn) -> bool {
        for attempt in 0..120 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let title = browser.eval_js("document.title").await;
            if let Ok(serde_json::Value::String(s)) = &title {
                if !s.is_empty() && s != "Just a moment..." {
                    log(&format!("ext: Cloudflare passed in {attempt}s ({s})"));
                    return true;
                }
            }
        }
        log(
            "ext: Cloudflare is still asking for a person after 120s -- open the \
             browser window and tick the box, then search again (ext needs a \
             visible browser on your own profile: a hidden one starts from a \
             clean profile every run)",
        );
        false
    }

    pub async fn ensure_logged_in(
        &self,
        cookie_file: Option<&Path>,
        username: Option<&str>,
        password: Option<&str>,
        log: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Result<bool> {
        let outcome = self
            .ensure_logged_in_inner(cookie_file, username, password, log.clone())
            .await;
        self.park().await;
        outcome
    }

    async fn ensure_logged_in_inner(
        &self,
        cookie_file: Option<&Path>,
        username: Option<&str>,
        password: Option<&str>,
        log: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Result<bool> {
        if self.logged_in.load(Ordering::Relaxed) {
            log("ext: already logged in (cached)");
            return Ok(true);
        }
        let browser = self.browser.lock().await;

        log("ext: opening the site for the login walk");
        browser.navigate(HOME_URL).await?;
        crate::browser::cloudflare::patch_cdp_detection(&browser)
            .await
            .ok();
        if !Self::wait_cloudflare(&browser, &log).await {
            return Ok(false);
        }

        if let Some(file) = cookie_file {
            match cookies::load_from_file(file) {
                Ok(saved) => {
                    // Only ext's own: the jar is one file for every source, and
                    // a browser handed another site's cookies answers an error.
                    let mine = cookies::for_domain(&saved, "ext.to");
                    if mine.is_empty() {
                        log(&format!("ext: no cookies of ours in {}", file.display()));
                    } else {
                        let json: Vec<serde_json::Value> =
                            mine.iter().map(|c| c.to_json()).collect();
                        // Not fatal: the credentials below can still type a
                        // session in, and a walk that gives up here reports a
                        // browser error instead of a login.
                        match browser.add_cookies(&json).await {
                            Ok(()) => log(&format!("ext: {} cookies injected", mine.len())),
                            Err(e) => log(&format!("ext: could not inject those cookies: {e}")),
                        }
                    }
                }
                Err(e) => log(&format!("ext: no cookies from {}: {e}", file.display())),
            }
        }

        // The question the whole source turns on: does the page carry a logout
        // link (a session) or the login modal (no session)?
        if browser
            .eval_js("!!document.querySelector(\"a[href*='logout']\")")
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            log("ext: already signed in");
            self.save_jar(&browser, cookie_file, &log).await;
            self.logged_in.store(true, Ordering::Relaxed);
            return Ok(true);
        }

        let (Some(user), Some(pass)) = (username, password) else {
            log(
                "ext: no credentials, and there is no session to reuse -- ext rows \
                 come back without a magnet until you sign in",
            );
            return Ok(false);
        };
        if user.is_empty() || pass.is_empty() {
            log("ext: credentials are empty, skipping the login");
            return Ok(false);
        }

        let script = fill_login_script(user, pass);
        log(&format!("ext: signing in as '{user}'"));
        match browser.eval_js(&script).await {
            Ok(serde_json::Value::String(answer)) => log(&format!("ext: form: {answer}")),
            Ok(other) => log(&format!("ext: form answered {other}")),
            Err(e) => {
                log(&format!("ext: the login form did not take: {e}"));
                return Ok(false);
            }
        }
        // The site's own handler posts over ajax, so there is no navigation to
        // wait for -- give it the seconds it takes, then look.
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        if let Ok(serde_json::Value::String(message)) = browser
            .eval_js("(document.querySelector('#auth-form .message-box')||{}).innerText || ''")
            .await
        {
            if !message.trim().is_empty() {
                log(&format!("ext: the site says: {}", message.trim()));
            }
        }
        let signed_in = browser
            .eval_js("!!document.querySelector(\"a[href*='logout']\")")
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !signed_in {
            log("ext: sign-in did not take -- wrong nickname or password?");
            return Ok(false);
        }
        self.logged_in.store(true, Ordering::Relaxed);
        self.save_jar(&browser, cookie_file, &log).await;
        Ok(true)
    }

    /// Put the browser's jar on disk, so the next run starts from a session
    /// instead of from a login form.
    ///
    /// This runs on the "already signed in" path too, and that is the point:
    /// ext's session can be sitting in the browser's own profile from a visit
    /// the user made themselves, and a jar doris never wrote is a session the
    /// next run has to ask the user for again.
    async fn save_jar(
        &self,
        browser: &Browser,
        cookie_file: Option<&Path>,
        log: &crate::sources::source::LogFn,
    ) {
        let Some(file) = cookie_file else {
            return;
        };
        match self.cookies_of(browser).await {
            Ok(jar) => match cookies::save_for_domain(file, "ext.to", &jar) {
                Ok(()) => log(&format!("ext: {} cookies saved", jar.len())),
                Err(e) => log(&format!("ext: could not save the cookies: {e}")),
            },
            Err(e) => log(&format!("ext: could not read the cookies: {e}")),
        }
    }

    /// The browser's cookies as the Netscape jar doris keeps on disk. The
    /// browser answers WebDriver's own JSON shape, which is not the jar's.
    async fn cookies_of(&self, browser: &Browser) -> Result<Vec<Cookie>> {
        let raw = browser.get_cookies().await?;
        let text = |c: &serde_json::Value, key: &str| {
            c.get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        Ok(raw
            .iter()
            .map(|c| Cookie {
                domain: text(c, "domain"),
                path: text(c, "path"),
                secure: c.get("secure").and_then(|v| v.as_bool()).unwrap_or(false),
                name: text(c, "name"),
                value: text(c, "value"),
            })
            .collect())
    }

    pub async fn search_page(
        &self,
        query: &str,
        offset: usize,
        category: Option<Group>,
    ) -> Result<SearchPage> {
        let browser = self.browser.lock().await;
        let url = search_url(query, offset, category);
        crate::log::log("search", &format!("ext: GET {url}"));
        browser.navigate(&url).await?;
        crate::browser::cloudflare::patch_cdp_detection(&browser)
            .await
            .ok();
        let log: crate::sources::source::LogFn = Arc::new(|m: &str| crate::log::log("ext", m));
        if !Self::wait_cloudflare(&browser, &log).await {
            bail!("ext: Cloudflare did not let the page through");
        }
        let scraped = browser.eval_js(ROWS_SCRIPT).await?;
        let text = scraped
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("ext: the row scrape returned no text"))?;
        let items = parse_rows(text)?;
        let has_more = items.len() >= PAGE_SIZE;
        Ok(SearchPage {
            items,
            has_more,
            next_offset: if has_more {
                Some(offset + PAGE_SIZE)
            } else {
                None
            },
        })
    }

    pub async fn resolve_magnet(&self, page_url: &str) -> Result<Option<String>> {
        let browser = self.browser.lock().await;
        let Some(id) = topic_id(page_url) else {
            bail!("ext: no topic id in {page_url}");
        };
        browser.navigate(page_url).await?;
        crate::browser::cloudflare::patch_cdp_detection(&browser)
            .await
            .ok();
        let log: crate::sources::source::LogFn = Arc::new(|m: &str| crate::log::log("ext", m));
        if !Self::wait_cloudflare(&browser, &log).await {
            bail!("ext: Cloudflare did not let the topic page through");
        }
        let answer = browser
            .eval_js(&MAGNET_SCRIPT.replace("__ID__", &id.to_string()))
            .await?;
        let text = answer
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("ext: the magnet ajax returned no text"))?;
        magnet_from_answer(text)
    }

    pub async fn download_torrent(&self, _url: &str) -> Result<Vec<u8>> {
        // Nothing to hand over: the rows carry neither a magnet nor a `.torrent`
        // URL, and both players of a row (Enter and `d`) ask for the magnet
        // first. Reaching this means a row arrived with a `.torrent` link from
        // somewhere else, which this source never writes.
        bail!("ext rows carry no .torrent URL -- they resolve to a magnet instead")
    }
}

/// The magnet out of what [`MAGNET_SCRIPT`] brought back.
///
/// The shape is awkward and worth pinning down in one place: the site answers
/// the POST with JSON, and the script kept the *text* of that answer, so there
/// is a JSON document inside a JSON string inside the script's own JSON. Both
/// endpoints are asked because which one answers depends on which page we are
/// standing on -- the wrong one answers **200** with "Invalid request. Please
/// refresh the page.", so only the payload tells the two apart.
pub fn magnet_from_answer(text: &str) -> Result<Option<String>> {
    #[derive(Deserialize)]
    struct Answer {
        token: bool,
        csrf: bool,
        #[serde(default)]
        answers: Vec<AnswerOne>,
    }
    #[derive(Deserialize)]
    struct AnswerOne {
        endpoint: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        error: Option<String>,
    }
    #[derive(Deserialize)]
    struct Payload {
        success: bool,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        error: Option<String>,
    }
    let answer: Answer =
        serde_json::from_str(text).context("ext: cannot read the magnet answer")?;
    if !answer.token || !answer.csrf {
        bail!(
            "ext: no session on the page (pageToken and csrf-token are only filled in \
               when signed in) -- sign in through the login modal first"
        );
    }
    for one in answer.answers {
        if let Some(err) = one.error {
            crate::log::log("ext", &format!("{}: {err}", one.endpoint));
            continue;
        }
        let payload: Payload = match serde_json::from_str(&one.text) {
            Ok(p) => p,
            Err(e) => {
                crate::log::log("ext", &format!("{}: unreadable answer: {e}", one.endpoint));
                continue;
            }
        };
        if let Some(url) = payload.url.filter(|u| u.starts_with("magnet:")) {
            return Ok(Some(url));
        }
        if !payload.success {
            crate::log::log(
                "ext",
                &format!(
                    "{}: {}",
                    one.endpoint,
                    payload.error.as_deref().unwrap_or("no url in the answer")
                ),
            );
        }
    }
    Ok(None)
}

/// The login form, filled the way a person fills it: through the native value
/// setter and with input/change events, because the site's handler reads the
/// fields through its own listeners.
fn fill_login_script(username: &str, password: &str) -> String {
    let escape = |value: &str| value.replace('\\', "\\\\").replace('\'', "\\'");
    format!(
        r#"
async function login() {{
  const form = document.querySelector('#auth-form');
  if (!form) return JSON.stringify({{ ok: false, error: 'no form on the page' }});
  const user = form.querySelector("input[name='name']");
  const pass = form.querySelector("input[name='password']");
  if (!user || !pass) return JSON.stringify({{ ok: false, error: 'no fields in the form' }});
  const set = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
  function fill(el, value) {{
    el.focus();
    set.call(el, value);
    el.dispatchEvent(new Event('input', {{ bubbles: true }}));
    el.dispatchEvent(new Event('change', {{ bubbles: true }}));
  }}
  fill(user, '{}');
  fill(pass, '{}');
  const button = form.querySelector('button[type=submit]');
  if (button) button.click();
  return JSON.stringify({{ ok: true, filled: user.value.length > 0 && pass.value.length > 0 }});
}}
return await login();
"#,
        escape(username),
        escape(password)
    )
}

#[async_trait]
impl Source for ExtSearcher {
    fn id(&self) -> &'static str {
        "ext"
    }

    fn label(&self) -> &'static str {
        "EXT"
    }

    fn groups(&self) -> &'static [Group] {
        EXT_GROUPS
    }

    fn home_url(&self) -> &'static str {
        HOME_URL
    }

    fn requires_browser(&self) -> bool {
        true
    }

    fn supports_browse(&self) -> bool {
        true
    }

    async fn ensure_logged_in(
        &self,
        auth: &super::source::AuthContext,
        log: &crate::sources::source::LogFn,
    ) -> Result<bool> {
        ExtSearcher::ensure_logged_in(
            self,
            auth.cookie_file.as_deref(),
            auth.username.as_deref(),
            auth.password.as_deref(),
            log.clone(),
        )
        .await
    }

    async fn search(&self, req: &SearchRequest) -> Result<SearchPage> {
        ExtSearcher::search_page(self, &req.query, req.offset, req.category).await
    }

    async fn download_torrent(&self, url: &str) -> Result<Vec<u8>> {
        ExtSearcher::download_torrent(self, url).await
    }

    async fn resolve_magnet(&self, page_url: &str) -> Result<Option<String>> {
        ExtSearcher::resolve_magnet(self, page_url).await
    }
}
