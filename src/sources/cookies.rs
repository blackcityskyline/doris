use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Cookie {
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub name: String,
    pub value: String,
}

impl Cookie {
    /// The payload `Browser::add_cookies` wants.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "value": self.value,
            "domain": self.domain.trim_start_matches('.'),
            "path": self.path,
            "secure": self.secure,
        })
    }
}

pub fn load_from_file(path: &Path) -> Result<Vec<Cookie>> {
    let content = std::fs::read_to_string(path)?;
    Ok(parse_netscape(&content))
}

/// The cookies of one site out of a jar that holds several.
///
/// The jar is one file for every source (`~/.config/doris/cookies.txt`), and
/// handing a browser cookies for a site it is not going to is not a no-op:
/// `add_cookies` answers an error for a domain the session was not opened on,
/// and the caller that ignores it loses the whole walk -- measured on 05.10.2026,
/// where signing into ext failed in ten seconds with rutracker's cookies in the
/// jar and succeeded in twenty-one with an empty one.
///
/// A leading dot means the cookie covers subdomains too, so `.rutracker.org`
/// belongs to `forum.rutracker.org` as well.
pub fn for_domain<'a>(cookies: &'a [Cookie], host: &str) -> Vec<&'a Cookie> {
    let host = host.trim_start_matches('.').to_ascii_lowercase();
    cookies
        .iter()
        .filter(|c| {
            let domain = c.domain.trim_start_matches('.').to_ascii_lowercase();
            !domain.is_empty() && (host == domain || host.ends_with(&format!(".{domain}")))
        })
        .collect()
}

pub fn save_to_file(path: &Path, cookies: &[Cookie]) -> Result<()> {
    write_jar(path, cookies)
}

/// Put one site's cookies into a jar that holds several, leaving the others
/// alone.
///
/// `save_to_file` writes the file it is given, which was fine while one tracker
/// was the only thing with a session and quietly stopped being true the moment a
/// second one logged in: signing into ext on 05.10.2026 replaced the file with
/// ext's six cookies and took rutracker's four with them, so the next rutracker
/// search asked for a password it had been given an hour earlier.
pub fn save_for_domain(path: &Path, host: &str, cookies: &[Cookie]) -> Result<()> {
    let mine = for_domain(cookies, host);
    let mut jar: Vec<Cookie> = load_from_file(path)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let key = |c: &Cookie| {
        (
            c.domain.trim_start_matches('.').to_ascii_lowercase(),
            c.name.clone(),
        )
    };
    let mine_keys: Vec<(String, String)> = mine.iter().map(|c| key(c)).collect();
    jar.retain(|c| !mine_keys.contains(&key(c)));
    jar.extend(mine.into_iter().cloned());
    write_jar(path, &jar)
}

fn write_jar(path: &Path, cookies: &[Cookie]) -> Result<()> {
    let mut content = String::from("# Netscape HTTP Cookie File\n");
    content.push_str("# https://curl.haxx.se/rfc/cookie_spec.html\n");
    content.push_str("# This is a generated file! Do not edit.\n\n");

    for cookie in cookies {
        let domain = if cookie.domain.starts_with('.') {
            cookie.domain.clone()
        } else {
            format!(".{}", cookie.domain)
        };
        let flag = if domain.starts_with('.') {
            "TRUE"
        } else {
            "FALSE"
        };
        let secure = if cookie.secure { "TRUE" } else { "FALSE" };
        content.push_str(&format!(
            "{}\t{}\t{}\t{}\t0\t{}\t{}\n",
            domain, flag, cookie.path, secure, cookie.name, cookie.value
        ));
    }

    crate::credentials::write_private(path, content.as_bytes())?;
    Ok(())
}

pub fn parse_netscape(content: &str) -> Vec<Cookie> {
    let mut cookies = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 7 {
            cookies.push(Cookie {
                domain: parts[0].to_string(),
                path: parts[2].to_string(),
                secure: parts[3] == "TRUE",
                name: parts[5].to_string(),
                value: parts[6].to_string(),
            });
        }
    }

    cookies
}
