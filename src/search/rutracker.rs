use anyhow::Result;
use crate::browser::cdp::Browser;
use crate::search::models::TorrentItem;
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
        Self {
            browser,
            logged_in: false,
        }
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

        // Check if the browser session is already authenticated (profile copy with cookies)
        {
            let browser = self.browser.lock().await;
            if !self.verify_login(&browser).await {
                // Try navigating to index.php first
                browser.navigate("https://rutracker.org/forum/index.php").await?;
                crate::browser::cloudflare::patch_cdp_detection(&browser).await.ok();
                Self::wait_cloudflare(&browser).await;
            }
            if self.verify_login(&browser).await {
                self.logged_in = true;
                return Ok(true);
            }
        }

        // Try loading from cookie file (only if explicitly provided)
        if let Some(cf) = cookie_file {
            if cf.exists() {
                let loaded_cookies = cookies::load_from_file(cf)?;
                if !loaded_cookies.is_empty() {
                    let browser = self.browser.lock().await;
                    for cookie in &loaded_cookies {
                        let cookie_json = serde_json::json!({
                            "name": cookie.name,
                            "value": cookie.value,
                            "domain": cookie.domain,
                            "path": cookie.path,
                            "secure": cookie.secure,
                        });
                        browser.add_cookies(&[cookie_json]).await?;
                    }

                    browser.navigate("https://rutracker.org/forum/index.php").await?;
                    Self::wait_cloudflare(&browser).await;

                    if self.verify_login(&browser).await {
                        self.logged_in = true;
                        return Ok(true);
                    }
                }
            }
        }

        // Try username/password login
        if let (Some(user), Some(pass)) = (username, password) {
            if self.login(user, pass).await? {
                self.logged_in = true;
                if let Some(cf) = cookie_file {
                    if let Ok(new_cookies) = self.get_cookies().await {
                        let _ = cookies::save_to_file(cf, &new_cookies);
                    }
                }
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn login(&self, username: &str, password: &str) -> Result<bool> {
        let browser = self.browser.lock().await;
        browser.navigate("https://rutracker.org/forum/index.php").await?;
        crate::browser::cloudflare::patch_cdp_detection(&browser).await.ok();
        Self::wait_cloudflare(&browser).await;

        let login_script = format!(
            r#"
            (async () => {{
                const userInput = document.querySelector("input[name='login_username'], #top_username");
                const passInput = document.querySelector("input[name='login_password'], #top_password");
                const loginBtn = document.querySelector("input[name='login'], #top_login-btn");

                if (userInput && passInput && loginBtn) {{
                    userInput.value = '{}';
                    passInput.value = '{}';
                    loginBtn.click();
                    return true;
                }}
                return false;
            }})()
            "#,
            username.replace('\'', "\\'"),
            password.replace('\'', "\\'")
        );

        let result = browser.eval_js(&login_script).await?;
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        Ok(result.as_bool().unwrap_or(false))
    }

    async fn verify_login(&self, browser: &Browser) -> bool {
        let script = r#"
        (() => {
            const logout = document.querySelector("a[href*='logout']");
            if (logout) return true;
            const profileLink = document.querySelector("a[href*='profile.php']");
            if (profileLink) return true;
            const topUsername = document.querySelector("[id='top-username'], .top_menu_username");
            if (topUsername && topUsername.textContent.trim().length > 0) return true;
            if (window.BB && !BB.IS_GUEST) return true;
            return false;
        })()
        "#;

        browser.eval_js(script).await.ok().and_then(|v| v.as_bool()).unwrap_or(false)
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

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36")
            .build()?;

        let mut req = client.get(url);
        let mut cookie_header = String::new();
        for c in &cookies {
            if !cookie_header.is_empty() {
                cookie_header.push_str("; ");
            }
            cookie_header.push_str(&format!("{}={}", c.name, c.value));
        }
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
            anyhow::bail!("Downloaded file is HTML page instead of .torrent. Session may not be logged in.");
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
        for i in 0..30 {
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
