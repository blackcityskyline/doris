use anyhow::Result;
use fantoccini::{ClientBuilder, Client};
use futures_util::{SinkExt, StreamExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserMode {
    Gui,
    Headless,
}

pub struct ExtractedCookies {
    pub profile_dir: PathBuf,
    pub cookies: Vec<serde_json::Value>,
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

pub struct Browser {
    client: Client,
    #[allow(dead_code)]
    child: Option<std::process::Child>,
}

impl Browser {
    pub async fn launch(binary: &Path, mode: BrowserMode) -> Result<Self> {
        let browser_major = detect_browser_major_version(binary)?;
        let chromedriver_path = get_or_patch_chromedriver(browser_major).await?;

        let extracted = extract_cookies_if_running(binary).await;

        let port = find_free_port()?;

        let mut cmd = std::process::Command::new(&chromedriver_path);
        cmd.arg(format!("--port={}", port))
            .arg("--silent")
            .stderr(Stdio::piped())
            .stdout(Stdio::null());

        let child = cmd.spawn()?;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

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

        let user_data_dir = extracted.as_ref()
            .map(|e| e.profile_dir.clone())
            .or_else(|| detect_user_data_dir(binary));

        if let Some(ref dir) = user_data_dir {
            chrome_args.push(format!("--user-data-dir={}", dir.display()));
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

        if let Some(ref ext) = extracted {
            if !ext.cookies.is_empty() {
                eprintln!("[browser] navigating to domain before injecting {} cookies", ext.cookies.len());
                let _ = client.goto("https://rutracker.org/forum/index.php").await;
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                inject_cookies_cdp(&client, &ext.cookies).await;
                let _ = client.goto("https://rutracker.org/forum/index.php").await;
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }

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
        let trimmed = script.trim_start();
        let wrapped = if trimmed.starts_with("return ") || trimmed.starts_with("throw ") || trimmed.starts_with("async ") {
            script.to_string()
        } else {
            let s = script.trim_end().trim_end_matches(';');
            format!("return {};", s)
        };
        Ok(self.client.execute(&wrapped, vec![]).await?)
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

async fn get_or_patch_chromedriver(browser_major: u32) -> Result<PathBuf> {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("t-hunter");
    std::fs::create_dir_all(&data_dir)?;

    let patched_path = data_dir.join("chromedriver_patched");

    if patched_path.exists() {
        return Ok(patched_path);
    }

    let chromedriver_path = find_or_download_chromedriver(browser_major).await?;
    let original = std::fs::read(&chromedriver_path)?;

    let patched = patch_chromedriver_binary(&original);

    std::fs::write(&patched_path, &patched)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&patched_path, std::fs::Permissions::from_mode(0o755))?;
    }

    eprintln!("[browser] Patched chromedriver -> {}", patched_path.display());
    Ok(patched_path)
}

fn patch_chromedriver_binary(content: &[u8]) -> Vec<u8> {
    let replacement = b"{console.log(\"undetected chromedriver 1337!\")}";
    let mut result = content.to_vec();

    let mut search_start = 0;
    while search_start < result.len() {
        if let Some(pos) = find_window_cdc_block(&result[search_start..]) {
            let abs_pos = search_start + pos;
            let block_len = measure_cdc_block(&result[abs_pos..]);
            if block_len > 0 {
                let end = (abs_pos + block_len).min(result.len());
                let actual_len = end - abs_pos;
                let patch: Vec<u8> = replacement
                    .iter()
                    .chain(std::iter::repeat(&b' ').take(actual_len.saturating_sub(replacement.len())))
                    .take(actual_len)
                    .copied()
                    .collect();
                result[abs_pos..end].copy_from_slice(&patch);
                search_start = abs_pos + actual_len;
                eprintln!("[browser] Patched cdc block at offset {}, length {}", abs_pos, actual_len);
            } else {
                search_start += pos + 1;
            }
        } else {
            break;
        }
    }

    result
}

fn find_window_cdc_block(data: &[u8]) -> Option<usize> {
    let marker = b"{window.cdc";
    let mut i = 0;
    while i + marker.len() <= data.len() {
        if &data[i..i + marker.len()] == marker {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn measure_cdc_block(data: &[u8]) -> usize {
    if !data.starts_with(b"{window.cdc") {
        return 0;
    }
    let mut depth = 0;
    for (i, &b) in data.iter().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    0
}

async fn find_or_download_chromedriver(browser_major: u32) -> Result<PathBuf> {
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

    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("t-hunter")
        .join("chromedriver");
    std::fs::create_dir_all(&data_dir)?;

    let downloaded = data_dir.join("chromedriver-linux64/chromedriver");

    if downloaded.exists() {
        return Ok(downloaded);
    }

    eprintln!("[browser] chromedriver not found, downloading for Chromium {}...", browser_major);

    let url = download_chromedriver_url(browser_major).await?;

    let status = std::process::Command::new("curl")
        .args(["-sL", "-o", "/tmp/chromedriver.zip", &url])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| anyhow::anyhow!("curl failed: {}", e))?;
    if !status.success() {
        anyhow::bail!("curl failed with status {}", status);
    }

    let status = std::process::Command::new("unzip")
        .args(["-o", "/tmp/chromedriver.zip", "-d", data_dir.to_str().unwrap()])
        .stdout(std::process::Stdio::null())
        .status()
        .map_err(|e| anyhow::anyhow!("unzip failed: {}", e))?;
    if !status.success() {
        anyhow::bail!("unzip failed with status {}", status);
    }

    if !downloaded.exists() {
        anyhow::bail!("chromedriver download failed: binary not found after extraction");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&downloaded, std::fs::Permissions::from_mode(0o755))?;
    }

    eprintln!("[browser] chromedriver downloaded -> {}", downloaded.display());
    Ok(downloaded)
}

async fn download_chromedriver_url(browser_major: u32) -> Result<String> {
    let client = reqwest::Client::new();
    let versions_url = "https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json";
    let resp: serde_json::Value = client.get(versions_url).send().await?.json().await?;

    let versions = resp["versions"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Invalid Chrome for Testing response"))?;

    for v in versions.iter().rev() {
        let version_str = v["version"].as_str().unwrap_or("");
        let major = version_str.split('.').next().and_then(|s| s.parse::<u32>().ok());
        if major != Some(browser_major) {
            continue;
        }

        if let Some(downloads) = v["downloads"]["chromedriver"].as_array() {
            for dl in downloads {
                if dl["platform"].as_str() == Some("linux64") {
                    let url = dl["url"].as_str().unwrap_or("");
                    if !url.is_empty() {
                        return Ok(url.to_string());
                    }
                }
            }
        }
    }

    anyhow::bail!(
        "No chromedriver found for Chromium {}. Download manually from https://googlechromelabs.github.io/chrome-for-testing/",
        browser_major
    )
}

fn read_devtools_port(profile_dir: &Path) -> Option<(u16, String)> {
    let port_file = profile_dir.join("DevToolsActivePort");
    let content = std::fs::read_to_string(&port_file).ok()?;
    let mut lines = content.lines();
    let port: u16 = lines.next()?.trim().parse().ok()?;
    let ws_path = lines.next()?.trim().to_string();
    Some((port, ws_path))
}

async fn extract_cookies_if_running(binary: &Path) -> Option<ExtractedCookies> {
    let home = dirs::home_dir()?;
    let bin_name = binary.file_name()?.to_str()?;
    let profile_names = ["helium", "brave", "chromium", "google-chrome"];

    for profile_name in &profile_names {
        if !bin_name.contains(profile_name) {
            continue;
        }
        let dirs_to_check = [
            home.join(".config").join(format!("net.imput.{}", profile_name)),
            home.join(".config").join(*profile_name),
            home.join(".config").join(format!("{}-browser", profile_name)),
        ];
        for config_dir in &dirs_to_check {
            if !config_dir.join("Default").exists() {
                continue;
            }
            let lock = config_dir.join("SingletonLock");
            if !lock.symlink_metadata().is_ok() {
                continue;
            }
            let (cdp_port, ws_path) = match read_devtools_port(&config_dir) {
                Some(v) => v,
                None => {
                    eprintln!("[browser] browser running but no DevToolsActivePort, can't extract cookies");
                    return None;
                }
            };
            eprintln!("[browser] found running browser: {} (cdp port={}, ws={})", config_dir.display(), cdp_port, &ws_path[..ws_path.len().min(40)]);

            let cookies = extract_cookies_via_cdp(cdp_port, &ws_path).await;
            if cookies.is_empty() {
                eprintln!("[browser] no cookies extracted, using native profile directly");
                return None;
            }

            eprintln!("[browser] extracted {} cookies via CDP", cookies.len());

            kill_browser_by_profile(&config_dir);
            std::thread::sleep(std::time::Duration::from_secs(2));
            let _ = std::fs::remove_file(&lock);

            return Some(ExtractedCookies {
                profile_dir: config_dir.clone(),
                cookies,
            });
        }
    }
    None
}

fn kill_browser_by_profile(profile_dir: &Path) {
    let profile_str = profile_dir.to_string_lossy();
    let output = std::process::Command::new("pgrep")
        .args(["-f", &format!("user-data-dir={}", profile_str)])
        .output();
    if let Ok(o) = output {
        for pid_str in String::from_utf8_lossy(&o.stdout).lines() {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                let _ = std::process::Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output();
                eprintln!("[browser] killed browser pid {}", pid);
            }
        }
    }
}

async fn extract_cookies_via_cdp(cdp_port: u16, _ws_path: &str) -> Vec<serde_json::Value> {
    let tabs_url = format!("http://127.0.0.1:{}/json", cdp_port);
    let tabs: serde_json::Value = match reqwest::get(&tabs_url).await {
        Ok(resp) => match resp.json().await {
            Ok(v) => v,
            Err(_) => return vec![],
        },
        Err(_) => return vec![],
    };

    let mut page_ws_url = String::new();
    if let Some(arr) = tabs.as_array() {
        for tab in arr {
            let tab_type = tab["type"].as_str().unwrap_or("");
            if tab_type == "page" {
                if let Some(ws) = tab["webSocketDebuggerUrl"].as_str() {
                    page_ws_url = ws.to_string();
                    break;
                }
            }
        }
    }
    if page_ws_url.is_empty() {
        eprintln!("[browser] no page tab found in CDP");
        return vec![];
    }

    eprintln!("[browser] connecting to page CDP: {}", &page_ws_url[page_ws_url.len()-40..]);
    match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio_tungstenite::connect_async(&page_ws_url)
    ).await {
        Err(_) => {
            eprintln!("[browser] CDP connect timeout");
            vec![]
        }
        Ok(Err(e)) => {
            eprintln!("[browser] CDP websocket connect failed: {}", e);
            vec![]
        }
        Ok(Ok((mut ws, _))) => {
            let enable_msg = serde_json::json!({"id": 0, "method": "Network.enable", "params": {}});
            let _ = ws.send(tokio_tungstenite::tungstenite::Message::Text(enable_msg.to_string())).await;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            while let Ok(Some(_)) = tokio::time::timeout(std::time::Duration::from_millis(100), ws.next()).await {}

            let msg = serde_json::json!({"id": 1, "method": "Network.getAllCookies", "params": {}});
            if ws.send(tokio_tungstenite::tungstenite::Message::Text(msg.to_string())).await.is_err() {
                return vec![];
            }
            while let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) = ws.next().await {
                if let Ok(resp) = serde_json::from_str::<serde_json::Value>(&text) {
                    if resp["id"] == serde_json::json!(1) {
                        return resp["result"]["cookies"]
                            .as_array()
                            .map(|arr| arr.iter().cloned().collect())
                            .unwrap_or_default();
                    }
                }
            }
            vec![]
        }
    }
}

async fn inject_cookies_cdp(client: &Client, cookies: &[serde_json::Value]) {
    let mut count = 0;
    for cookie in cookies {
        let name = cookie.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let value = cookie.get("value").and_then(|v| v.as_str()).unwrap_or("");
        let domain = cookie.get("domain").and_then(|v| v.as_str()).unwrap_or("");

        if name.is_empty() || domain.is_empty() {
            continue;
        }
        if !domain.contains("rutracker") {
            continue;
        }

        let mut builder = fantoccini::cookies::Cookie::build((name.to_string(), value.to_string()));
        builder = builder.domain(domain.to_string());

        if let Some(p) = cookie.get("path").and_then(|v| v.as_str()) {
            builder = builder.path(p.to_string());
        }
        if let Some(s) = cookie.get("secure").and_then(|v| v.as_bool()) {
            builder = builder.secure(s);
        }
        if let Some(h) = cookie.get("httpOnly").and_then(|v| v.as_bool()) {
            builder = builder.http_only(h);
        }

        match client.add_cookie(builder.build()).await {
            Ok(()) => count += 1,
            Err(e) => eprintln!("[browser]   failed to set {}: {}", name, e),
        }
    }
    eprintln!("[browser] injected {} rutracker cookies via fantoccini", count);
}

fn detect_user_data_dir(binary: &Path) -> Option<PathBuf> {
    let bin_name = binary.file_name()?.to_str()?;
    let home = dirs::home_dir()?;

    let profile_names = ["helium", "brave", "chromium", "google-chrome", "google-chrome-stable"];

    for profile_name in &profile_names {
        if !bin_name.contains(profile_name) {
            continue;
        }
        let dirs_to_check = [
            home.join(".config").join(format!("net.imput.{}", profile_name)),
            home.join(".config").join(*profile_name),
            home.join(".config").join(format!("{}-browser", profile_name)),
        ];
        for config_dir in &dirs_to_check {
            if config_dir.join("Default").exists() {
                eprintln!("[browser] using user profile: {}", config_dir.display());
                return Some(config_dir.clone());
            }
        }
    }

    None
}

fn detect_browser_major_version(binary: &Path) -> Result<u32> {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run {}: {}", binary.display(), e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let version_str = if !stdout.trim().is_empty() { stdout } else { stderr };

    let version_str = version_str.trim();

    for part in version_str.split_whitespace().rev() {
        if let Some(major) = part.split('.').next().and_then(|s| s.parse::<u32>().ok()) {
            if major > 10 {
                return Ok(major);
            }
        }
    }

    anyhow::bail!(
        "Could not detect browser version from '{}' ({})",
        binary.display(),
        version_str
    )
}


