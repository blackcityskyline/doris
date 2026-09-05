use anyhow::Result;
use crate::browser::cdp::Browser;
use crate::search::models::TorrentItem;
use crate::search::cookies::{self, Cookie};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

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
        cookie_file: &Path,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<bool> {
        if self.logged_in {
            return Ok(true);
        }

        if cookie_file.exists() {
            let loaded_cookies = cookies::load_from_file(cookie_file)?;
            if !loaded_cookies.is_empty() {
                let browser = self.browser.lock().await;
                browser.navigate("https://rutracker.org/forum/index.php").await?;
                crate::browser::cloudflare::patch_cdp_detection(&browser).await.ok();
                Self::wait_cloudflare(&browser).await;

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

        if let (Some(user), Some(pass)) = (username, password) {
            if self.login(user, pass).await? {
                self.logged_in = true;
                if let Ok(new_cookies) = self.get_cookies().await {
                    let _ = cookies::save_to_file(cookie_file, &new_cookies);
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
            const cookies = document.cookie.split(';').reduce((acc, c) => {
                const parts = c.trim().split('=');
                if (parts.length >= 2) acc[parts[0]] = parts[1];
                return acc;
            }, {});
            if (cookies['bb_data'] || (cookies['bb_session'] && !cookies['bb_session'].startsWith('0-'))) {
                return true;
            }
            const logout = document.querySelector("a[href*='logout']");
            if (logout) return true;
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
        // Inject anti-detection patches into the new page
        crate::browser::cloudflare::patch_cdp_detection(&browser).await.ok();
        // Wait for Cloudflare to pass and page to load (up to 30s)
        Self::wait_cloudflare(&browser).await;

        let page_source = browser.get_page_source().await.unwrap_or_default();
        eprintln!("[debug] page source length: {}", page_source.len());
        if page_source.contains("login.php") {
            eprintln!("[debug] redirect to login detected");
            return Ok(Vec::new());
        }
        // Save page source for debugging
        let _ = std::fs::write("/tmp/t-hunter-debug.html", &page_source);
        eprintln!("[debug] saved page source to /tmp/t-hunter-debug.html");
        // Check if tor-tbl exists
        let has_table = browser.eval_js("!!document.querySelector('#tor-tbl')").await;
        eprintln!("[debug] has #tor-tbl: {:?}", has_table);
        // Check page title
        let title = browser.eval_js("document.title").await;
        eprintln!("[debug] page title: {:?}", title);

        let parse_script = r#"
        (() => {
            const results = [];
            const rows = document.querySelectorAll('#tor-tbl tr');
            for (const row of rows) {
                const titleLink = row.querySelector('a.tLink');
                if (!titleLink) continue;
                const title = titleLink.textContent.trim();
                const href = titleLink.getAttribute('href');
                if (!href) continue;
                const downloadUrl = href.replace('viewtopic.php', 'dl.php');

                const sizeCell = row.querySelector('td.tor-size');
                const seedsCell = row.querySelector('td.seedmed');
                const dateCell = row.querySelector('td.t-date');

                results.push({
                    title,
                    size: sizeCell ? sizeCell.textContent.trim() : '',
                    seeds: seedsCell ? seedsCell.textContent.trim() : '',
                    date: dateCell ? dateCell.textContent.trim() : '',
                    download_url: downloadUrl,
                    page_url: href,
                });
            }
            return JSON.stringify(results);
        })()
        "#;

        let result = browser.eval_js(parse_script).await?;
        let json_str = result.as_str().unwrap_or("[]");
        eprintln!("[debug] parsed json length: {}", json_str.len());
        if json_str.len() < 200 {
            eprintln!("[debug] parsed: {}", json_str);
        }
        let items: Vec<TorrentItem> = serde_json::from_str(json_str)?;
        Ok(items)
    }

    pub async fn download_torrent(&self, url: &str) -> Result<Vec<u8>> {
        let browser = self.browser.lock().await;
        let js_code = format!(
            r#"
            (async () => {{
                const response = await fetch('{}');
                if (!response.ok) {{
                    throw new Error('HTTP ' + response.status);
                }}
                const buffer = await response.arrayBuffer();
                const bytes = new Uint8Array(buffer);
                let binary = '';
                for (let i = 0; i < bytes.byteLength; i++) {{
                    binary += String.fromCharCode(bytes[i]);
                }}
                return btoa(binary);
            }})()
            "#,
            url.replace('\'', "\\'")
        );

        let result = browser.eval_js(&js_code).await?;
        let b64 = result.as_str().ok_or_else(|| anyhow::anyhow!("No data returned"))?;
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)?;

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
                    eprintln!("[debug] Cloudflare passed after {}s, title: {}", i, s);
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    return;
                }
            }
            eprintln!("[debug] waiting for Cloudflare... ({}/30)", i + 1);
        }
    }
}
