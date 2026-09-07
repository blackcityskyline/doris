use anyhow::Result;
use crate::browser::cdp::Browser;
use crate::search::models::{TorrentItem, resolve_url};
use crate::search::cookies::{self, Cookie};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct RutrackerSearcher {
    browser: Arc<Mutex<Browser>>,
    logged_in: bool,
}

impl RutrackerSearcher {
    pub fn new(browser: Arc<Mutex<Browser>>) -> Self {
        Self { browser, logged_in: false }
    }

    pub async fn ensure_logged_in(
        &mut self,
        cookie_file: Option<&Path>,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<bool> {
        if self.logged_in {
            return Ok(true);
        }

        let browser = self.browser.lock().await;

        // Step 1: Load cookies from file and inject them
        if let Some(cf) = cookie_file {
            if cf.exists() {
                match cookies::load_from_file(cf) {
                    Ok(loaded) if !loaded.is_empty() => {
                        crate::log::log("auth", &format!("loaded {} cookies from {}", loaded.len(), cf.display()));
                        let json_cookies: Vec<serde_json::Value> = loaded.iter().map(|c| {
                            serde_json::json!({
                                "name": c.name,
                                "value": c.value,
                                "domain": c.domain,
                                "path": c.path,
                                "secure": c.secure,
                            })
                        }).collect();
                        browser.add_cookies(&json_cookies).await?;
                    }
                    _ => {}
                }
            }
        }

        // Step 2: Navigate to rutracker and pass Cloudflare
        browser.navigate("https://rutracker.org/forum/index.php").await?;
        crate::browser::cloudflare::patch_cdp_detection(&browser).await.ok();
        Self::wait_cloudflare(&browser).await;

        // Step 3: Check if cookies were enough
        if self.verify_login(&browser).await {
            self.logged_in = true;
            crate::log::log("auth", "logged in via cookies");
            return Ok(true);
        }

        // Step 4: Not logged in - need to login with credentials
        crate::log::log("auth", "cookies didn't work, need login");
        drop(browser);

        if let (Some(user), Some(pass)) = (username, password) {
            if !user.is_empty() && !pass.is_empty() {
                crate::log::log("auth", &format!("attempting login as '{}'", user));
                let result = self.login(user, pass).await;
                match result {
                    Ok(true) => {
                        self.logged_in = true;
                        // Save cookies after successful login
                        if let Some(cf) = cookie_file {
                            match self.get_cookies().await {
                                Ok(c) => {
                                    let _ = cookies::save_to_file(cf, &c);
                                    crate::log::log("auth", &format!("saved {} cookies to {}", c.len(), cf.display()));
                                }
                                Err(e) => crate::log::log("auth", &format!("failed to save cookies: {}", e)),
                            }
                        }
                        return Ok(true);
                    }
                    Ok(false) => {
                        crate::log::log("auth", "login failed - wrong credentials or form not found");
                    }
                    Err(e) => {
                        crate::log::log("auth", &format!("login error: {}", e));
                    }
                }
            }
        }

        Ok(false)
    }

    async fn login(&self, username: &str, password: &str) -> Result<bool> {
        let browser = self.browser.lock().await;

        // Navigate to login page
        browser.navigate("https://rutracker.org/forum/login.php").await?;
        crate::browser::cloudflare::patch_cdp_detection(&browser).await.ok();
        Self::wait_cloudflare(&browser).await;

        let username_escaped = username.replace('\\', "\\\\").replace('\'', "\\'");
        let password_escaped = password.replace('\\', "\\\\").replace('\'', "\\'");

        // Synchronous JS - no IIFE, no async. eval_js will wrap in "return ..."
        let login_script = format!(
            r#"(() => {{
                const u = document.querySelector("input[name='login_username'], input[name='username'], #top_username, #login-username");
                const p = document.querySelector("input[name='login_password'], input[name='password'], #top_password, #login-password");
                const b = document.querySelector("input[name='login'], #top_login-btn, input.login_btn, input[type='submit']");
                if (!u || !p) return JSON.stringify({{ok:false, error:'no_form', url:location.href}});
                u.focus(); u.value='{}'; u.dispatchEvent(new Event('input',{{bubbles:true}})); u.dispatchEvent(new Event('change',{{bubbles:true}}));
                p.focus(); p.value='{}'; p.dispatchEvent(new Event('input',{{bubbles:true}})); p.dispatchEvent(new Event('change',{{bubbles:true}}));
                if (b) b.click(); else {{ const f = u.closest('form'); if (f) f.submit(); }}
                return JSON.stringify({{ok:true, user:u.name||u.id, pass:p.name||p.id, hasBtn:!!b}});
            }})()"#,
            username_escaped, password_escaped
        );

        let result = browser.eval_js(&login_script).await?;
        let result_str = result.as_str().unwrap_or("{}");
        crate::log::log("auth", &format!("login script: {}", result_str));

        if result_str.contains("no_form") {
            return Ok(false);
        }

        // Wait for page to load after form submit
        for _ in 0..10 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let cookies = browser.get_cookies().await.unwrap_or_default();
            let has_session = cookies.iter().any(|c| {
                let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let value = c.get("value").and_then(|v| v.as_str()).unwrap_or("");
                let domain = c.get("domain").and_then(|v| v.as_str()).unwrap_or("");
                domain.contains("rutracker") && (name == "bb_data" || name == "bb_session") && !value.is_empty()
            });
            if has_session {
                crate::log::log("auth", "session cookies found after login");
                return Ok(true);
            }

            // Check if we're on the forum (login succeeded and redirected)
            let url = browser.eval_js("location.href").await
                .map(|v| v.as_str().unwrap_or("").to_string())
                .unwrap_or_default();
            if url.contains("index.php") || url.contains("tracker.php") || url.contains("viewtopic.php") {
                // We're on a forum page - check for logout link
                let html = browser.get_page_source().await.unwrap_or_default();
                if html.contains("logout.php") {
                    crate::log::log("auth", "login succeeded (redirected to forum, logout link found)");
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    async fn verify_login(&self, browser: &Browser) -> bool {
        // Check CDP cookies for session
        if let Ok(cookies) = browser.get_cookies().await {
            let has_session = cookies.iter().any(|c| {
                let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let value = c.get("value").and_then(|v| v.as_str()).unwrap_or("");
                let domain = c.get("domain").and_then(|v| v.as_str()).unwrap_or("");
                domain.contains("rutracker") && (name == "bb_data" || name == "bb_session") && !value.is_empty()
            });
            if has_session {
                crate::log::log("auth", "verify: session cookies present");
                return true;
            }
        }

        // Check HTML for logout link (definitive logged-in indicator)
        if let Ok(html) = browser.get_page_source().await {
            if html.contains("logout.php") {
                crate::log::log("auth", "verify: logout.php link found in HTML");
                return true;
            }
        }

        crate::log::log("auth", "verify: not logged in");
        false
    }

    pub async fn search(&self, query: &str) -> Result<Vec<TorrentItem>> {
        let browser = self.browser.lock().await;
        let encoded_query = urlencoding::encode(query);
        let search_url = format!(
            "https://rutracker.org/forum/tracker.php?nm={}&o=10&s=2",
            encoded_query
        );

        browser.navigate(&search_url).await?;
        crate::browser::cloudflare::patch_cdp_detection(&browser).await.ok();
        Self::wait_cloudflare(&browser).await;

        let url = browser.eval_js("location.href").await
            .map(|v| v.as_str().unwrap_or("").to_string())
            .unwrap_or_default();

        if url.contains("login.php") {
            crate::log::log("search", "redirected to login page - not authenticated");
            return Ok(vec![]);
        }

        let parse_script = r#"
        return JSON.stringify(Array.from(document.querySelectorAll('#tor-tbl tbody tr')).map(row => {
            const titleLink = row.querySelector('a.tLink');
            if (!titleLink) return null;
            const title = titleLink.textContent.trim();
            const href = titleLink.getAttribute('href');
            if (!href) return null;
            const downloadUrl = href.replace('viewtopic.php', 'dl.php');
            const sizeCell = row.querySelector('td.tor-size');
            const seedsEl = row.querySelector('b.seedmed');
            const dlLink = row.querySelector('a.dl-stub');
            const dateCells = row.querySelectorAll('td[data-ts_text]');
            let date = '';
            for (const dc of dateCells) {
                const p = dc.querySelector('p');
                if (p) { date = p.textContent.trim(); break; }
            }
            return {
                title,
                size: dlLink ? dlLink.textContent.trim() : (sizeCell ? sizeCell.getAttribute('data-ts_text') || '' : ''),
                seeds: seedsEl ? seedsEl.textContent.trim() : '',
                date,
                download_url: dlLink ? dlLink.getAttribute('href') || downloadUrl : downloadUrl,
                page_url: href,
            };
        }).filter(x => x !== null));
        "#;

        let result = browser.eval_js(parse_script).await?;
        let json_str = result.as_str().unwrap_or("[]");
        let items: Vec<TorrentItem> = serde_json::from_str(json_str)?;
        crate::log::log("search", &format!("found {} results for '{}'", items.len(), query));
        Ok(items)
    }

    pub async fn download_torrent(&self, url: &str) -> Result<Vec<u8>> {
        let cookies = self.get_cookies().await?;
        let full_url = resolve_url(url);

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36")
            .build()?;

        let mut cookie_header = String::new();
        for c in &cookies {
            if !cookie_header.is_empty() {
                cookie_header.push_str("; ");
            }
            cookie_header.push_str(&format!("{}={}", c.name, c.value));
        }

        let mut req = client.get(&full_url)
            .header("Referer", "https://rutracker.org/forum/index.php");

        if !cookie_header.is_empty() {
            req = req.header("Cookie", cookie_header);
        }

        let resp = req.send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await?.to_vec();

        if !status.is_success() {
            anyhow::bail!("HTTP {} downloading torrent", status);
        }

        let lower = bytes[..200.min(bytes.len())].to_ascii_lowercase();
        if bytes.starts_with(b"<!DOCTYPE") || lower.windows(5).any(|w| w == b"html") {
            anyhow::bail!("Downloaded HTML instead of .torrent. Session may not be logged in.");
        }

        Ok(bytes)
    }

    async fn get_cookies(&self) -> Result<Vec<Cookie>> {
        let browser = self.browser.lock().await;
        let raw_cookies = browser.get_cookies().await?;
        let mut result = Vec::new();
        for c in raw_cookies {
            result.push(Cookie {
                domain: c.get("domain").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                path: c.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                secure: c.get("secure").and_then(|v| v.as_bool()).unwrap_or(false),
                name: c.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                value: c.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            });
        }
        Ok(result)
    }

    async fn wait_cloudflare(browser: &Browser) {
        for _ in 0..30 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let title = browser.eval_js("document.title").await;
            if let Ok(serde_json::Value::String(s)) = &title {
                if !s.is_empty() && s != "Just a moment..." {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    return;
                }
            }
        }
    }
}
