//! Magnet link parsing and building (ROADMAP.md B7), ported from torio's
//! `magnet.ts`.
//!
//! Why this exists: a result row that carries a magnet can go to
//! TorrServer as a *link* -- no `.torrent` download, no source-side
//! round trip, and the download starts from the DHT instead of waiting
//! for one host to hand over a file. `parse_magnet`/`parse_input` also
//! normalize every hash to 40-char hex, because that is the one form all
//! of our sources, TorrServer and the torrent itself agree on.
//!
//! Everything here is a plain function, so `tests/magnet_tests.rs` ports
//! torio's `magnet.test.ts` case for case.

/// torio's `TRACKERS`: seven public trackers appended to any magnet we
/// build ourselves, so a bare hash still has peers to ask when the row
/// did not come with trackers of its own.
pub const TRACKERS: [&str; 7] = [
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://tracker.openbittorrent.com:6969/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.dler.org:6969/announce",
];

/// A magnet reduced to the three things a caller actually wants: a
/// normalized 40-char hex hash, a human name, and the link as given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMagnet {
    pub info_hash: String,
    pub name: String,
    pub magnet: String,
}

/// torio's `buildMagnet`: percent-encoded display name plus the seven
/// [`TRACKERS`], each as its own `tr=` parameter.
pub fn build_magnet(info_hash: &str, name: &str) -> String {
    let mut magnet = format!(
        "magnet:?xt=urn:btih:{}&dn={}",
        info_hash,
        encode_component(name)
    );
    for tracker in TRACKERS {
        magnet.push_str("&tr=");
        magnet.push_str(encode_component(tracker).as_str());
    }
    magnet
}

/// torio's `normalizeInfoHash`: a 32-char base32 hash decodes to 40 hex
/// chars; anything else is just lowercased (a hex hash is always
/// reported lowercase by every source we speak to, but a hash that is
/// neither valid base32 nor hex at that length survives untouched rather
/// than being invented into something wrong).
pub fn normalize_info_hash(raw: &str) -> String {
    if raw.len() == 32 {
        if let Some(hex) = base32_to_hex(raw) {
            return hex;
        }
    }
    raw.to_lowercase()
}

/// torio's `parseMagnet`: a `magnet:?` URI containing an `xt=urn:btih:`
/// with a 40-char hex or 32-char base32 hash. Anything else -- a plain
/// string, a malformed hash, text that merely *contains* a magnet after
/// a prefix -- is `None`.
pub fn parse_magnet(input: &str) -> Option<ParsedMagnet> {
    let s = input.trim();
    // `get(..8)` rather than a slice: a byte index past a multibyte
    // character would panic, and this takes arbitrary user text.
    let has_scheme = s
        .get(..8)
        .map(|p| p.eq_ignore_ascii_case("magnet:?"))
        .unwrap_or(false);
    if !has_scheme {
        return None;
    }

    // torio's MAGNET_RE, without a regex engine: find the xt parameter,
    // then take the hash-shaped run after it.
    let prefix = "xt=urn:btih:";
    let at = s.to_ascii_lowercase().find(prefix)? + prefix.len();
    let run: String = s[at..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();

    let info_hash = if run.len() >= 40 && run[..40].chars().all(|c| c.is_ascii_hexdigit()) {
        run[..40].to_string()
    } else if run.len() >= 32 && is_base32_alphabet(&run[..32]) {
        run[..32].to_string()
    } else {
        return None;
    };
    let info_hash = normalize_info_hash(&info_hash);

    // `dn` when present, the hash itself when it is not (torio does the
    // same: an absent *or empty* display name falls back to the hash).
    let name = query_param(s, "dn").unwrap_or_else(|| info_hash.clone());
    Some(ParsedMagnet {
        info_hash,
        name,
        magnet: s.to_string(),
    })
}

/// torio's `isInfoHash`, anchored: only a string that is *nothing but* a
/// 40-char hex or 32-char base32 hash counts, so an ordinary search
/// query like `the office 1080p` is never mistaken for one.
pub fn is_info_hash(input: &str) -> bool {
    let s = input.trim();
    if s.len() == 40 {
        return s.chars().all(|c| c.is_ascii_hexdigit());
    }
    if s.len() == 32 {
        return is_base32_alphabet(s);
    }
    false
}

/// torio's `parseInput`: a magnet URI, or a bare hash wrapped with the
/// default [`TRACKERS`] so it downloads over DHT like any other magnet.
/// `None` for anything that is neither.
pub fn parse_input(input: &str) -> Option<ParsedMagnet> {
    let s = input.trim();
    if let Some(parsed) = parse_magnet(s) {
        return Some(parsed);
    }
    if !is_info_hash(s) {
        return None;
    }
    let info_hash = normalize_info_hash(s);
    let magnet = build_magnet(&info_hash, &info_hash);
    Some(ParsedMagnet {
        name: info_hash.clone(),
        magnet,
        info_hash,
    })
}

/// The `base32` alphabet torio uses, checked case-insensitively: A-Z and
/// 2-7 only (`0`, `1`, `8`, `9` are not base32).
fn is_base32_alphabet(value: &str) -> bool {
    value
        .chars()
        .all(|c| c.is_ascii_alphabetic() || ('2'..='7').contains(&c))
}

/// torio's `base32ToHex`: `None` unless the result is exactly 40 hex
/// chars, i.e. unless the input really was a 32-char base32 hash.
fn base32_to_hex(b32: &str) -> Option<String> {
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut bits = 0u32;
    let mut value = 0u32;
    let mut out = String::with_capacity(40);
    for c in b32.chars() {
        let index = ALPHABET.find(c.to_ascii_uppercase())? as u32;
        value = (value << 5) | index;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push_str(&format!("{:02x}", (value >> bits) & 0xff));
            value &= (1 << bits) - 1;
        }
    }
    (out.len() == 40).then_some(out)
}

/// `key=value` from the query part of a magnet, percent-decoded with
/// `+` read as a space (what `URLSearchParams` does for us in torio).
fn query_param(input: &str, key: &str) -> Option<String> {
    let question = input.find('?')? + 1;
    for pair in input[question..].split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if name != key {
            continue;
        }
        let decoded = percent_decode(value);
        // An empty `dn=` is not a name; fall back to the hash.
        return (!decoded.is_empty()).then_some(decoded);
    }
    None
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                match (hi, lo) {
                    (Some(hi), Some(lo)) => {
                        out.push((hi * 16 + lo) as u8);
                        i += 3;
                    }
                    // Not an escape after all -- keep the `%` literally
                    // rather than eating a character.
                    _ => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `encodeURIComponent` as torio calls it: percent-encode everything
/// outside the unreserved set, so a display name with `&`, `?` or `=`
/// cannot smuggle extra parameters into the magnet.
fn encode_component(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}
