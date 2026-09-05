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

pub fn load_from_file(path: &Path) -> Result<Vec<Cookie>> {
    let content = std::fs::read_to_string(path)?;
    Ok(parse_netscape(&content))
}

pub fn save_to_file(path: &Path, cookies: &[Cookie]) -> Result<()> {
    let mut content = String::from("# Netscape HTTP Cookie File\n");
    content.push_str("# https://curl.haxx.se/rfc/cookie_spec.html\n");
    content.push_str("# This is a generated file! Do not edit.\n\n");

    for cookie in cookies {
        let domain = if cookie.domain.starts_with('.') {
            cookie.domain.clone()
        } else {
            format!(".{}", cookie.domain)
        };
        let flag = if domain.starts_with('.') { "TRUE" } else { "FALSE" };
        let secure = if cookie.secure { "TRUE" } else { "FALSE" };
        content.push_str(&format!(
            "{}\t{}\t{}\t{}\t0\t{}\t{}\n",
            domain, flag, &cookie.path, secure, cookie.name, cookie.value
        ));
    }

    std::fs::write(path, content)?;
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
