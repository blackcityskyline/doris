use anyhow::Result;
use chromiumoxide::browser::{Browser as CdmBrowser, BrowserConfig};
use chromiumoxide::page::Page;
use chromiumoxide_cdp::cdp::browser_protocol::network::{Cookie, CookieParam};
use futures_lite::StreamExt;
use std::path::Path;

pub struct Browser {
    #[allow(dead_code)]
    browser: CdmBrowser,
    page: Page,
}

impl Browser {
    pub async fn launch(binary: &Path, headless: bool) -> Result<Self> {
        let mut builder = BrowserConfig::builder()
            .chrome_executable(binary)
            .no_sandbox()
            .arg("--disable-dev-shm-usage".to_string())
            .arg("--window-size=1920,1080".to_string());

        if headless {
            builder = builder.new_headless_mode();
        }

        let (browser, mut handler) = CdmBrowser::launch(builder.build().map_err(|e| anyhow::anyhow!("{}", e))?).await?;

        tokio::spawn(async move {
            while let Some(_) = handler.next().await {}
        });

        let page = browser.new_page("about:blank").await?;

        Ok(Self { browser, page })
    }

    pub async fn navigate(&self, url: &str) -> Result<()> {
        self.page.goto(url).await?;
        Ok(())
    }

    pub async fn get_page_source(&self) -> Result<String> {
        let html = self.page.content().await?;
        Ok(html)
    }

    pub async fn eval_js(&self, script: &str) -> Result<serde_json::Value> {
        let result = self.page.evaluate(script).await?;
        let value: serde_json::Value = result.into_value()?;
        Ok(value)
    }

    #[allow(dead_code)]
    pub async fn wait_for_selector(&self, selector: &str, timeout_ms: u64) -> Result<()> {
        let script = format!(
            r#"
            new Promise((resolve, reject) => {{
                const start = Date.now();
                const check = () => {{
                    if (document.querySelector('{}')) {{
                        resolve(true);
                    }} else if (Date.now() - start > {}) {{
                        reject(new Error('Timeout waiting for selector'));
                    }} else {{
                        setTimeout(check, 100);
                    }}
                }};
                check();
            }})
            "#,
            selector.replace('\'', "\\'"),
            timeout_ms
        );
        self.page.evaluate(script.as_str()).await?;
        Ok(())
    }

    pub async fn get_cookies(&self) -> Result<Vec<Cookie>> {
        let cookies = self.page.get_cookies().await?;
        Ok(cookies)
    }

    pub async fn add_cookies(&self, cookies: &[serde_json::Value]) -> Result<()> {
        for cookie in cookies {
            let name = cookie.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let value = cookie.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let domain = cookie.get("domain").and_then(|v| v.as_str()).map(|s| s.to_string());
            let path = cookie.get("path").and_then(|v| v.as_str()).map(|s| s.to_string());

            let param = CookieParam {
                name,
                value,
                url: None,
                domain,
                path,
                secure: cookie.get("secure").and_then(|v| v.as_bool()),
                http_only: None,
                same_site: None,
                expires: None,
                priority: None,
                same_party: None,
                source_scheme: None,
                source_port: None,
                partition_key: None,
            };

            self.page.set_cookie(param).await?;
        }
        Ok(())
    }
}
