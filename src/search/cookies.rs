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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_netscape_basic() {
        let content = "# Netscape HTTP Cookie File\n\
            .rutracker.org\tTRUE\t/\tTRUE\t0\tbb_session\tabc123\n\
            .rutracker.org\tTRUE\t/\tFALSE\t0\tbb_data\txyz789\n";
        let cookies = parse_netscape(content);
        assert_eq!(cookies.len(), 2);
        assert_eq!(cookies[0].name, "bb_session");
        assert_eq!(cookies[0].value, "abc123");
        assert_eq!(cookies[0].domain, ".rutracker.org");
        assert_eq!(cookies[0].path, "/");
        assert!(cookies[0].secure);
        assert_eq!(cookies[1].name, "bb_data");
        assert!(!cookies[1].secure);
    }

    #[test]
    fn test_parse_netscape_skips_comments() {
        let content = "# This is a comment\n# Another comment\n.rutracker.org\tTRUE\t/\tTRUE\t0\tsession\tval\n";
        let cookies = parse_netscape(content);
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].name, "session");
    }

    #[test]
    fn test_parse_netscape_skips_empty_lines() {
        let content = "\n\n\n.rutracker.org\tTRUE\t/\tTRUE\t0\tsession\tval\n\n\n";
        let cookies = parse_netscape(content);
        assert_eq!(cookies.len(), 1);
    }

    #[test]
    fn test_parse_netscape_short_line_ignored() {
        let content = "too\tfew\tcolumns\n";
        let cookies = parse_netscape(content);
        assert_eq!(cookies.len(), 0);
    }

    #[test]
    fn test_parse_netscape_cf_clearance() {
        let content = ".rutracker.org\tTRUE\t/\tTRUE\t0\tcf_clearance\tdef456\n\
            .rutracker.org\tTRUE\t/\tFALSE\t0\tbb_guid\tguid123\n\
            .rutracker.org\tTRUE\t/\tFALSE\t0\tbb_ssl\t1\n";
        let cookies = parse_netscape(content);
        assert_eq!(cookies.len(), 3);
        assert_eq!(cookies[0].name, "cf_clearance");
        assert_eq!(cookies[1].name, "bb_guid");
        assert_eq!(cookies[2].name, "bb_ssl");
    }

    #[test]
    fn test_parse_netscape_empty() {
        let cookies = parse_netscape("");
        assert_eq!(cookies.len(), 0);
    }

    #[test]
    fn test_save_and_reload() {
        let dir = std::env::temp_dir().join("t-hunter-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_cookies.txt");

        let cookies = vec![
            Cookie {
                domain: "rutracker.org".to_string(),
                path: "/".to_string(),
                secure: true,
                name: "bb_session".to_string(),
                value: "test123".to_string(),
            },
            Cookie {
                domain: ".rutracker.org".to_string(),
                path: "/forum".to_string(),
                secure: false,
                name: "cf_clearance".to_string(),
                value: "cf456".to_string(),
            },
        ];

        save_to_file(&path, &cookies).unwrap();
        let loaded = load_from_file(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "bb_session");
        assert_eq!(loaded[0].value, "test123");
        assert!(loaded[0].domain.starts_with('.'));
        assert_eq!(loaded[1].name, "cf_clearance");

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
