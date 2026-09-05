use anyhow::Result;
use fantoccini::{ClientBuilder, Client};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserMode {
    Gui,
    Headless,
}

impl std::fmt::Display for BrowserMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrowserMode::Gui => write!(f, "gui"),
            BrowserMode::Headless => write!(f, "headless"),
        }
    }
}

impl std::str::FromStr for BrowserMode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "gui" | "window" | "visible" => Ok(BrowserMode::Gui),
            "headless" | "hidden" | "bg" => Ok(BrowserMode::Headless),
            _ => anyhow::bail!("Unknown browser mode '{}'. Use 'gui' or 'headless'", s),
        }
    }
}

const CDC_MARKER: &[u8] = b"cdc_";

pub struct Browser {
    client: Client,
    #[allow(dead_code)]
    child: Option<std::process::Child>,
}

impl Browser {
    pub async fn launch(binary: &Path, mode: BrowserMode) -> Result<Self> {
        let chromedriver_path = get_or_patch_chromedriver()?;

        let port = find_free_port()?;

        let mut cmd = std::process::Command::new(&chromedriver_path);
        cmd.arg("--port")
            .arg(port.to_string())
            .arg("--silent")
            .arg("--hide-scrollbars")
            .stderr(Stdio::piped())
            .stdout(Stdio::null());

        let child = cmd.spawn()?;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let mut chrome_args: Vec<String> = vec![
            "--no-sandbox".into(),
            "--disable-dev-shm-usage".into(),
            "--disable-blink-features=AutomationControlled".into(),
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
            "--disable-infobars".into(),
            "--disable-background-timer-throttling".into(),
            "--disable-backgrounding-occluded-windows".into(),
            "--disable-renderer-backgrounding".into(),
            "--lang=ru-RU".into(),
        ];

        if mode == BrowserMode::Headless {
            chrome_args.push("--headless=new".into());
        } else {
            chrome_args.push("--window-size=1920,1080".into());
        }

        let mut capabilities = serde_json::Map::new();
        let chrome_opts = serde_json::json!({
            "binary": binary.to_str().unwrap_or_default(),
            "args": chrome_args,
            "excludeSwitches": ["enable-automation"],
            "useAutomationExtension": false
        });
        capabilities.insert("goog:chromeOptions".into(), chrome_opts);

        let webdriver_url = format!("http://127.0.0.1:{}", port);

        let client = ClientBuilder::native()
            .capabilities(capabilities)
            .connect(&webdriver_url)
            .await?;

        eprintln!("[browser] {} via patched chromedriver on port {}", mode, port);

        Ok(Self {
            client,
            child: Some(child),
        })
    }

    pub async fn navigate(&self, url: &str) -> Result<()> {
        self.client.goto(url).await?;
        Ok(())
    }

    pub async fn get_page_source(&self) -> Result<String> {
        Ok(self.client.source().await?)
    }

    pub async fn eval_js(&self, script: &str) -> Result<serde_json::Value> {
        Ok(self.client.execute(script, vec![]).await?)
    }

    pub async fn get_cookies(&self) -> Result<Vec<serde_json::Value>> {
        let cookies = self.client.get_all_cookies().await?;
        let values: Vec<serde_json::Value> = cookies
            .into_iter()
            .map(|c| {
                let mut map = serde_json::Map::new();
                map.insert("name".into(), c.name().into());
                map.insert("value".into(), c.value().into());
                map.insert("domain".into(), c.domain().unwrap_or_default().into());
                map.insert("path".into(), c.path().unwrap_or_default().into());
                map.insert("secure".into(), c.secure().into());
                map.insert("httpOnly".into(), c.http_only().into());
                serde_json::Value::Object(map)
            })
            .collect();
        Ok(values)
    }

    pub async fn add_cookie(&self, cookie: fantoccini::cookies::Cookie<'static>) -> Result<()> {
        self.client.add_cookie(cookie).await?;
        Ok(())
    }

    pub async fn add_cookies(&self, cookies: &[serde_json::Value]) -> Result<()> {
        for c in cookies {
            let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let value = c.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut builder = fantoccini::cookies::Cookie::build((name, value));
            if let Some(d) = c.get("domain").and_then(|v| v.as_str()) {
                builder = builder.domain(d.to_string());
            }
            if let Some(p) = c.get("path").and_then(|v| v.as_str()) {
                builder = builder.path(p.to_string());
            }
            if let Some(s) = c.get("secure").and_then(|v| v.as_bool()) {
                builder = builder.secure(s);
            }
            if let Some(h) = c.get("httpOnly").and_then(|v| v.as_bool()) {
                builder = builder.http_only(h);
            }
            let cookie = builder.build();
            self.client.add_cookie(cookie).await?;
        }
        Ok(())
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
        }
    }
}

fn find_free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn get_or_patch_chromedriver() -> Result<PathBuf> {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("t-hunter");
    std::fs::create_dir_all(&data_dir)?;

    let patched_path = data_dir.join("chromedriver_patched");

    if patched_path.exists() {
        return Ok(patched_path);
    }

    let chromedriver_path = find_chromedriver()?;
    let original = std::fs::read(&chromedriver_path)?;

    if !original.windows(4).any(|w| w == CDC_MARKER) {
        std::fs::copy(&chromedriver_path, &patched_path)?;
        return Ok(patched_path);
    }

    let mut patched = original;
    let mut i = 0;
    while i + 4 <= patched.len() {
        if &patched[i..i + 4] == CDC_MARKER {
            for j in i..i + 4 {
                patched[j] = b'_';
            }
        }
        i += 1;
    }

    std::fs::write(&patched_path, &patched)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&patched_path, std::fs::Permissions::from_mode(0o755))?;
    }

    eprintln!("[browser] Patched chromedriver -> {}", patched_path.display());
    Ok(patched_path)
}

fn find_chromedriver() -> Result<PathBuf> {
    if let Ok(path) = which::which("chromedriver") {
        return Ok(path);
    }

    for path in &[
        "/usr/lib/chromium/chromedriver",
        "/usr/bin/chromedriver",
        "/snap/chromium/current/usr/lib/chromium-browser/chromedriver",
    ] {
        if Path::new(path).exists() {
            return Ok(PathBuf::from(path));
        }
    }

    anyhow::bail!("chromedriver not found. Install: pacman -S chromedriver")
}

pub type SharedBrowser = Arc<Mutex<Browser>>;

pub async fn create_browser(binary: &Path, mode: BrowserMode) -> Result<SharedBrowser> {
    let browser = Browser::launch(binary, mode).await?;
    Ok(Arc::new(Mutex::new(browser)))
}
