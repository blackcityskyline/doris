use anyhow::Result;
use fantoccini::{ClientBuilder, Client};
use std::path::{Path, PathBuf};
use std::process::Stdio;

#[derive(Debug, Clone, PartialEq, Eq)]
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

pub struct Browser {
    client: Client,
    child: Option<std::process::Child>,
    temp_profile: Option<PathBuf>,
    xvfb_child: Option<std::process::Child>,
}

impl Browser {
    pub async fn launch(binary: &Path, mode: BrowserMode) -> Result<Self> {
        let browser_major = detect_browser_major_version(binary)?;
        let chromedriver_path = get_or_patch_chromedriver(browser_major).await?;

        let native_profile = detect_user_data_dir(binary);
        let mut injected_cookies: Vec<serde_json::Value> = Vec::new();

        let temp_profile = if mode == BrowserMode::Headless {
            let tmp = std::env::temp_dir().join(format!("doris-headless-{}", std::process::id()));
            std::fs::create_dir_all(&tmp)?;
            crate::log::log("browser", &format!("headless: temp profile {}", tmp.display()));

            if let Some(ref native) = native_profile {
                match extract_cookies_from_native_profile(native) {
                    Ok(cookies) => {
                        crate::log::log("browser", &format!("extracted {} cookies from native profile", cookies.len()));
                        injected_cookies = cookies;
                    }
                    Err(e) => {
                        crate::log::log("browser", &format!("could not extract native cookies: {}", e));
                    }
                }
            }
            Some(tmp)
        } else {
            if let Some(ref dir) = native_profile {
                graceful_shutdown_if_running(dir);
            }
            native_profile.clone()
        };

        let use_xvfb = mode == BrowserMode::Headless && has_xvfb();
        let mut xvfb_child = None;
        if use_xvfb {
            crate::log::log("browser", "using xvfb virtual display");
        }

        let port = find_free_port()?;

        let mut cmd = if use_xvfb {
            let display_num = find_free_display();
            xvfb_child = start_xvfb(display_num);
            let mut c = std::process::Command::new(&chromedriver_path);
            c.env("DISPLAY", format!(":{}", display_num));
            c
        } else {
            std::process::Command::new(&chromedriver_path)
        };

        cmd.arg(format!("--port={}", port))
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

        if mode == BrowserMode::Headless && !use_xvfb {
            chrome_args.push("--headless=new".into());
        }

        if use_xvfb {
            chrome_args.push("--ozone-platform=x11".into());
        }

        chrome_args.push("--window-size=1920,1080".into());

        if let Some(ref dir) = temp_profile {
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

        crate::log::log("browser", &format!("{} via patched chromedriver on port {}", mode, port));

        let browser = Self {
            client,
            child: Some(child),
            temp_profile: if mode == BrowserMode::Headless { temp_profile } else { None },
            xvfb_child,
        };

        if !injected_cookies.is_empty() {
            crate::log::log("browser", "navigating to domain for cookie injection...");
            browser.navigate("https://rutracker.org/forum/index.php").await.ok();
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            crate::log::log("browser", &format!("injecting {} cookies into headless session", injected_cookies.len()));
            browser.add_cookies(&injected_cookies).await?;
        }

        Ok(browser)
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
        if let Some(ref mut xvfb) = self.xvfb_child {
            let _ = xvfb.kill();
        }
        if let Some(ref path) = self.temp_profile {
            crate::log::log("browser", &format!("cleanup temp profile {}", path.display()));
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

fn find_free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn has_xvfb() -> bool {
    std::process::Command::new("which")
        .arg("Xvfb")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn find_free_display() -> u32 {
    for num in 99..200 {
        let sock = std::path::PathBuf::from(format!("/tmp/.X11-unix/X{}", num));
        if !sock.exists() {
            return num;
        }
    }
    99
}

fn start_xvfb(display_num: u32) -> Option<std::process::Child> {
    let child = std::process::Command::new("Xvfb")
        .args(&[
            &format!(":{}", display_num),
            "-screen", "0", "1920x1080x24",
            "-nolisten", "tcp",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    std::thread::sleep(std::time::Duration::from_secs(1));
    crate::log::log("browser", &format!("started Xvfb on :{}", display_num));
    Some(child)
}

fn graceful_shutdown_if_running(profile_dir: &Path) {
    let lock = profile_dir.join("SingletonLock");
    if !lock.symlink_metadata().is_ok() {
        return;
    }

    crate::log::log("browser", "shutting down browser...");

    let pids = find_pids_by_profile(profile_dir);
    if pids.is_empty() {
        let _ = std::fs::remove_file(&lock);
        return;
    }

    for pid in &pids {
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .output();
    }
    crate::log::log("browser", &format!("sent SIGTERM to {} processes", pids.len()));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if !lock.symlink_metadata().is_ok() {
            crate::log::log("browser", "browser exited cleanly");
            return;
        }
        if std::time::Instant::now() >= deadline {
            crate::log::log("browser", "timeout, sending SIGKILL");
            for pid in &pids {
                let _ = std::process::Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output();
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
            let _ = std::fs::remove_file(&lock);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

fn find_pids_by_profile(profile_dir: &Path) -> Vec<u32> {
    let profile_str = profile_dir.to_string_lossy();
    let output = match std::process::Command::new("pgrep")
        .args(["-f", &format!("user-data-dir={}", profile_str)])
        .output() {
            Ok(o) => o,
            Err(_) => return vec![],
        };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect()
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
                crate::log::log("browser", &format!("profile: {}", config_dir.display()));
                return Some(config_dir.clone());
            }
        }
    }

    None
}

async fn get_or_patch_chromedriver(browser_major: u32) -> Result<PathBuf> {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("doris");
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

    crate::log::log("browser", &format!("patched chromedriver -> {}", patched_path.display()));
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
                crate::log::log("browser", &format!("patched cdc block at offset {}, length {}", abs_pos, actual_len));
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
        .join("doris")
        .join("chromedriver");
    std::fs::create_dir_all(&data_dir)?;

    let downloaded = data_dir.join("chromedriver-linux64/chromedriver");

    if downloaded.exists() {
        return Ok(downloaded);
    }

    crate::log::log("browser", &format!("downloading chromedriver for Chromium {}...", browser_major));

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

    crate::log::log("browser", &format!("chromedriver downloaded -> {}", downloaded.display()));
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

fn extract_cookies_from_native_profile(profile_dir: &Path) -> Result<Vec<serde_json::Value>> {
    let cookie_paths = [
        profile_dir.join("Default/Cookies"),
        profile_dir.join("Default/Network/Cookies"),
        profile_dir.join("Default/Cookies-journal"),
        profile_dir.join("Default/Network/Cookies-journal"),
    ];

    let cookie_db = cookie_paths.iter().find(|p| p.exists() && p.file_name().map(|n| n == "Cookies").unwrap_or(false))
        .ok_or_else(|| anyhow::anyhow!("No Cookies database found in profile"))?;

    let tmp_copy = std::env::temp_dir().join(format!("doris-cookies-{}.sqlite", std::process::id()));
    std::fs::copy(cookie_db, &tmp_copy)?;

    let conn = rusqlite::Connection::open(&tmp_copy)?;
    let mut stmt = conn.prepare(
        "SELECT host_key, name, value, path, is_secure, is_httponly, encrypted_value FROM cookies WHERE host_key LIKE '%rutracker%'"
    )?;

    let rows = stmt.query_map([], |row| {
        let host: String = row.get(0)?;
        let name: String = row.get(1)?;
        let value: String = row.get(2)?;
        let path: String = row.get(3)?;
        let secure: bool = row.get(4)?;
        let http_only: bool = row.get(5)?;
        let encrypted: Vec<u8> = row.get(6)?;
        Ok((host, name, value, path, secure, http_only, encrypted))
    })?;

    let mut cookies = Vec::new();
    for row in rows {
        let (host, name, value, path, secure, http_only, encrypted_value) = row?;

        let final_value = if !value.is_empty() {
            value
        } else if !encrypted_value.is_empty() {
            continue;
        } else {
            continue;
        };

        let domain = if host.starts_with('.') {
            host.clone()
        } else {
            format!(".{}", host)
        };

        cookies.push(serde_json::json!({
            "name": name,
            "value": final_value,
            "domain": domain,
            "path": path,
            "secure": secure,
            "httpOnly": http_only,
        }));
    }

    let _ = std::fs::remove_file(&tmp_copy);
    Ok(cookies)
}
