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
        log: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Result<bool> {
        if self.logged_in {
            log("AUTH: already logged in (cached)");
            return Ok(true);
        }

        let browser = self.browser.lock().await;

        // Step 1: Load cookies from file
        if let Some(cf) = cookie_file {
            if cf.exists() {
                match cookies::load_from_file(cf) {
                    Ok(loaded) if !loaded.is_empty() => {
                        log(&format!("AUTH: loaded {} cookies from {}", loaded.len(), cf.display()));
                        let json_cookies: Vec<serde_json::Value> = loaded.iter().map(|c| {
                            serde_json::json!({
                                "name": c.name,
                                "value": c.value,
                                "domain": c.domain,
                                "path": c.path,
                                "secure": c.secure,
                            })
                        }).collect();
                        if let Err(e) = browser.add_cookies(&json_cookies).await {
                            log(&format!("AUTH: failed to inject cookies: {}", e));
                        } else {
                            log("AUTH: cookies injected into browser");
                        }
                    }
                    Ok(_) => {
                        log(&format!("AUTH: cookie file exists but empty: {}", cf.display()));
                    }
                    Err(e) => {
                        log(&format!("AUTH: failed to read cookie file {}: {}", cf.display(), e));
                    }
                }
            } else {
                log(&format!("AUTH: no cookie file at {}", cf.display()));
            }
        } else {
            log("AUTH: no cookie file specified");
        }

        // Step 2: Navigate and pass Cloudflare
        log("AUTH: navigating to rutracker.org...");
        if let Err(e) = browser.navigate("https://rutracker.org/forum/index.php").await {
            log(&format!("AUTH: failed to navigate: {}", e));
            return Err(e);
        }
        log("AUTH: patching Cloudflare detection...");
        crate::browser::cloudflare::patch_cdp_detection(&browser).await.ok();
        log("AUTH: waiting for Cloudflare challenge...");
        Self::wait_cloudflare(&browser).await;

        let current_url = browser.eval_js("location.href").await
            .map(|v| v.as_str().unwrap_or("").to_string())
            .unwrap_or_default();
        log(&format!("AUTH: current URL: {}", current_url));

        let page_title = browser.eval_js("document.title").await
            .map(|v| v.as_str().unwrap_or("").to_string())
            .unwrap_or_default();
        log(&format!("AUTH: page title: {}", page_title));

        // Step 3: Check if logged in
        if self.verify_login(&browser).await {
            self.logged_in = true;
            log("AUTH: VERIFIED - logged in via cookies!");
            return Ok(true);
        }

        log("AUTH: not logged in yet, checking what we have...");
        self.log_session_state(&browser, &log).await;
        drop(browser);

        // Step 4: Need to login
        if let (Some(user), Some(pass)) = (username, password) {
            if !user.is_empty() && !pass.is_empty() {
                log(&format!("AUTH: attempting login as '{}'...", user));
                match self.login(user, pass, log.clone()).await {
                    Ok(true) => {
                        self.logged_in = true;
                        log("AUTH: LOGIN SUCCESSFUL!");
                        if let Some(cf) = cookie_file {
                            match self.get_cookies().await {
                                Ok(c) => {
                                    let _ = cookies::save_to_file(cf, &c);
                                    log(&format!("AUTH: saved {} cookies to {}", c.len(), cf.display()));
                                }
                                Err(e) => log(&format!("AUTH: failed to save cookies: {}", e)),
                            }
                        }
                        return Ok(true);
                    }
                    Ok(false) => {
                        log("AUTH: LOGIN FAILED - wrong credentials, form not found, or verification failed");
                    }
                    Err(e) => {
                        log(&format!("AUTH: LOGIN ERROR: {}", e));
                    }
                }
            } else {
                log("AUTH: credentials provided but empty, skipping login");
            }
        } else {
            log("AUTH: no credentials provided, cannot login");
        }

        Ok(false)
    }

    async fn log_session_state(&self, browser: &Browser, log: &Arc<dyn Fn(&str) + Send + Sync>) {
        let cookies = browser.get_cookies().await.unwrap_or_default();
        let rutracker_cookies: Vec<_> = cookies.iter().filter(|c| {
            c.get("domain").and_then(|v| v.as_str()).unwrap_or("").contains("rutracker")
        }).collect();

        log(&format!("AUTH: total cookies: {}, rutracker cookies: {}", cookies.len(), rutracker_cookies.len()));
        for c in &rutracker_cookies {
            let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let val = c.get("value").and_then(|v| v.as_str()).unwrap_or("");
            let domain = c.get("domain").and_then(|v| v.as_str()).unwrap_or("?");
            log(&format!("AUTH:   cookie: {}={} (domain: {}, len: {})", name, &val[..val.len().min(20)], domain, val.len()));
        }

        let html = browser.get_page_source().await.unwrap_or_default();
        let has_login_form = html.contains("login_username") || html.contains("login_password") || html.contains("Введите ваше имя");
        let has_logout = html.contains("logout.php");
        log(&format!("AUTH: HTML has login_form={} has_logout={}", has_login_form, has_logout));

        if html.len() < 5000 {
            log(&format!("AUTH: page HTML ({} bytes): {}", html.len(), &html[..html.len().min(300)]));
        }
    }

    async fn login(&self, username: &str, password: &str, log: Arc<dyn Fn(&str) + Send + Sync>) -> Result<bool> {
        let browser = self.browser.lock().await;

        log("AUTH LOGIN: navigating to login.php...");
        browser.navigate("https://rutracker.org/forum/login.php").await?;
        crate::browser::cloudflare::patch_cdp_detection(&browser).await.ok();
        log("AUTH LOGIN: waiting for Cloudflare...");
        Self::wait_cloudflare(&browser).await;

        let url = browser.eval_js("location.href").await
            .map(|v| v.as_str().unwrap_or("").to_string())
            .unwrap_or_default();
        log(&format!("AUTH LOGIN: current URL: {}", url));

        let title = browser.eval_js("document.title").await
            .map(|v| v.as_str().unwrap_or("").to_string())
            .unwrap_or_default();
        log(&format!("AUTH LOGIN: page title: {}", title));

        // Check form exists before trying
        let form_check = browser.eval_js(
            "JSON.stringify({inputs: document.querySelectorAll('input').length, forms: document.querySelectorAll('form').length, loginUser: !!document.querySelector(\"input[name='login_username'], input[name='username'], #top_username, #login-username\"), loginPass: !!document.querySelector(\"input[name='login_password'], input[name='password'], #top_password, #login-password\")})"
        ).await?;
        log(&format!("AUTH LOGIN: form check: {}", form_check.as_str().unwrap_or("?")));

        if form_check.as_str().unwrap_or("").contains("loginUser\":false") {
            log("AUTH LOGIN: ERROR - username input NOT FOUND on page");
            let snippet = browser.eval_js("document.body ? document.body.innerText.substring(0, 500) : 'no body'")
                .await.map(|v| v.as_str().unwrap_or("").to_string()).unwrap_or_default();
            log(&format!("AUTH LOGIN: page text: {}", snippet));
            return Ok(false);
        }

        let username_escaped = username.replace('\\', "\\\\").replace('\'', "\\'");
        let password_escaped = password.replace('\\', "\\\\").replace('\'', "\\'");

        let login_script = format!(
            r#"(() => {{
                const u = document.querySelector("input[name='login_username'], input[name='username'], #top_username, #login-username");
                const p = document.querySelector("input[name='login_password'], input[name='password'], #top_password, #login-password");
                if (!u || !p) return JSON.stringify({{ok:false, error:'no_form', url:location.href}});

                const nativeSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
                function fillField(el, val) {{
                    el.focus();
                    if (nativeSetter) nativeSetter.call(el, val);
                    else el.value = val;
                    el.dispatchEvent(new Event('input', {{bubbles:true}}));
                    el.dispatchEvent(new Event('change', {{bubbles:true}}));
                    el.dispatchEvent(new KeyboardEvent('keyup', {{bubbles:true, key:'a'}}));
                }}

                fillField(u, '{}');
                fillField(p, '{}');

                const form = u.closest('form');
                if (form) {{
                    form.submit();
                }} else {{
                    const btn = document.querySelector("input[type='submit']");
                    if (btn) btn.click();
                }}

                return JSON.stringify({{ok:true, uVal:u.value.substring(0,3), pLen:p.value.length, formAction: form ? form.action : 'none'}});
            }})()"#,
            username_escaped, password_escaped
        );

        let result = browser.eval_js(&login_script).await?;
        let result_str = result.as_str().unwrap_or("{}");
        log(&format!("AUTH LOGIN: script result: {}", result_str));

        if result_str.contains("no_form") {
            log("AUTH LOGIN: ERROR - login form not found after fill attempt");
            return Ok(false);
        }

        if let Some(u_val) = result_str.split("\"uVal\":\"").nth(1) {
            let u_val: &str = u_val.split('"').next().unwrap_or("?");
            log(&format!("AUTH LOGIN: username field value starts with: '{}'", u_val));
        }
        if let Some(p_len) = result_str.split("\"pLen\":").nth(1) {
            let p_len: &str = p_len.split(',').next().unwrap_or("?");
            log(&format!("AUTH LOGIN: password field length: {}", p_len));
        }

        // Wait for navigation and session
        log("AUTH LOGIN: waiting for form submission result...");
        for i in 0..15 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            let url = browser.eval_js("location.href").await
                .map(|v| v.as_str().unwrap_or("").to_string())
                .unwrap_or_default();

            let cookies = browser.get_cookies().await.unwrap_or_default();
            let session_cookie = cookies.iter().find(|c| {
                let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let domain = c.get("domain").and_then(|v| v.as_str()).unwrap_or("");
                domain.contains("rutracker") && (name == "bb_data" || name == "bb_session")
            });

            match session_cookie {
                Some(c) => {
                    let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let val = c.get("value").and_then(|v| v.as_str()).unwrap_or("");
                    log(&format!("AUTH LOGIN: [{}s] session cookie FOUND: {} (len={})", i+1, name, val.len()));
                    return Ok(true);
                }
                None => {
                    log(&format!("AUTH LOGIN: [{}s] URL={} cookies(rutr={})", i+1,
                        &url[..url.len().min(80)],
                        cookies.iter().filter(|c| c.get("domain").and_then(|v| v.as_str()).unwrap_or("").contains("rutracker")).count()
                    ));
                }
            }

            if url.contains("index.php") || url.contains("tracker.php") {
                let html = browser.get_page_source().await.unwrap_or_default();
                if html.contains("logout.php") {
                    log(&format!("AUTH LOGIN: [{}s] on forum page with logout link - LOGIN OK", i+1));
                    return Ok(true);
                } else {
                    log(&format!("AUTH LOGIN: [{}s] on forum page but NO logout link", i+1));
                }
            }
        }

        log("AUTH LOGIN: TIMEOUT - no session after 15s");
        Ok(false)
    }

    async fn verify_login(&self, browser: &Browser) -> bool {
        if let Ok(cookies) = browser.get_cookies().await {
            let has_session = cookies.iter().any(|c| {
                let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let value = c.get("value").and_then(|v| v.as_str()).unwrap_or("");
                let domain = c.get("domain").and_then(|v| v.as_str()).unwrap_or("");
                domain.contains("rutracker") && (name == "bb_data" || name == "bb_session") && !value.is_empty()
            });
            if has_session {
                return true;
            }
        }

        if let Ok(html) = browser.get_page_source().await {
            if html.contains("logout.php") {
                return true;
            }
        }

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
