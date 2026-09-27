//! Magnet tests ported from torio's `magnet.test.ts`, case for case,
//! plus the one case the port makes *load-bearing* here: a row whose
//! magnet is malformed must not be handed to TorrServer.

use doris::sources::magnet::{
    TRACKERS, build_magnet, is_info_hash, normalize_info_hash, parse_input, parse_magnet,
};

const HEX_HASH: &str = "abcdef0123456789abcdef0123456789abcdef01";
const BASE32_HASH: &str = "MFRGGZDFMZTWQ2LKNNWG23TPOBYXE43U";

// --- parse_magnet ------------------------------------------------------------

#[test]
fn test_parse_magnet_keeps_a_full_40_char_hex_hash_and_decodes_dn() {
    let parsed = parse_magnet(&format!("magnet:?xt=urn:btih:{}&dn=Cool+Movie", HEX_HASH))
        .expect("a well-formed magnet parses");

    assert_eq!(parsed.info_hash, HEX_HASH);
    assert_eq!(parsed.info_hash.len(), 40);
    assert_eq!(parsed.name, "Cool Movie", "`+` is a space, like URLSearchParams");
}

#[test]
fn test_parse_magnet_decodes_a_32_char_base32_hash_to_hex() {
    let parsed = parse_magnet(&format!(
        "magnet:?xt=urn:btih:{}&dn=X",
        BASE32_HASH
    ))
    .expect("base32 hashes parse too");

    assert_eq!(parsed.info_hash.len(), 40);
    assert!(
        parsed.info_hash.chars().all(|c| c.is_ascii_hexdigit()),
        "expected 40 hex chars, got {}",
        parsed.info_hash
    );
    assert_eq!(parsed.name, "X");
}

#[test]
fn test_parse_magnet_falls_back_to_the_hash_as_name_without_dn() {
    let parsed = parse_magnet(&format!("magnet:?xt=urn:btih:{}", HEX_HASH))
        .expect("a trackerless magnet still parses");

    assert_eq!(parsed.name, parsed.info_hash);
    assert_eq!(parsed.magnet, format!("magnet:?xt=urn:btih:{}", HEX_HASH));
}

#[test]
fn test_parse_magnet_rejects_non_magnets_and_malformed_hashes() {
    assert_eq!(parse_magnet("not a magnet"), None);
    assert_eq!(parse_magnet("magnet:?xt=urn:btih:tooshort"), None);
    assert_eq!(
        parse_magnet(&format!("prefix magnet:?xt=urn:btih:{}", "a".repeat(40))),
        None,
        "the scheme has to be at the front"
    );
    assert_eq!(parse_magnet(""), None);
}

// --- normalize_info_hash -----------------------------------------------------

#[test]
fn test_normalize_lowercases_40_char_hex() {
    assert_eq!(
        normalize_info_hash("ABCDEF0123456789ABCDEF0123456789ABCDEF01"),
        HEX_HASH
    );
}

#[test]
fn test_normalize_decodes_32_char_base32_to_hex() {
    let normalized = normalize_info_hash(BASE32_HASH);

    assert!(
        normalized.chars().all(|c| c.is_ascii_hexdigit()) && normalized.len() == 40,
        "expected 40 hex chars, got {}",
        normalized
    );
}

// --- build_magnet ------------------------------------------------------------

#[test]
fn test_build_magnet_encodes_the_name_and_appends_trackers() {
    let out = build_magnet("abc123", "My Movie 2024");

    assert!(out.contains("xt=urn:btih:abc123"), "{}", out);
    assert!(out.contains("dn=My%20Movie%202024"), "{}", out);
    assert!(out.contains("&tr="), "{}", out);
    assert_eq!(out.matches("&tr=").count(), TRACKERS.len(), "all 7 trackers");
}

#[test]
fn test_build_magnet_escapes_parameter_separators_in_the_name() {
    // Otherwise a title with `&dn=` in it would invent a second
    // parameter for TorrServer to read.
    let out = build_magnet(HEX_HASH, "Tom & Jerry");

    assert!(out.contains("dn=Tom%20%26%20Jerry"), "{}", out);
    assert!(!out.contains("Jerry&dn"), "one `dn` only: {}", out);
}

// --- is_info_hash ------------------------------------------------------------

#[test]
fn test_is_info_hash_accepts_bare_hashes_only() {
    assert!(is_info_hash(&"a".repeat(40)), "bare 40-char hex");
    assert!(is_info_hash(BASE32_HASH), "bare 32-char base32");
    assert!(is_info_hash(&format!("  {}  ", HEX_HASH)), "whitespace trimmed");
}

#[test]
fn test_is_info_hash_rejects_queries_and_malformed_hashes() {
    assert!(!is_info_hash("the office 1080p"));
    assert!(!is_info_hash(&"g".repeat(40)), "40 chars but not hex");
    assert!(!is_info_hash(&"a".repeat(39)), "too short");
    assert!(!is_info_hash(""));
}

// --- parse_input -------------------------------------------------------------

#[test]
fn test_parse_input_parses_a_full_magnet_like_parse_magnet() {
    let parsed = parse_input(&format!("magnet:?xt=urn:btih:{}&dn=Cool+Movie", HEX_HASH))
        .expect("a magnet is a magnet to parse_input too");

    assert_eq!(parsed.info_hash, HEX_HASH);
    assert_eq!(parsed.name, "Cool Movie");
}

#[test]
fn test_parse_input_wraps_a_bare_hash_into_a_magnet_with_trackers() {
    let parsed = parse_input(HEX_HASH).expect("a bare hex hash is accepted");

    assert_eq!(parsed.info_hash, HEX_HASH);
    assert_eq!(parsed.name, HEX_HASH);
    assert!(parsed.magnet.contains(&format!("xt=urn:btih:{}", HEX_HASH)));
    assert!(parsed.magnet.contains("&tr="), "bare hashes need trackers");
}

#[test]
fn test_parse_input_decodes_a_bare_base32_hash_and_trims_it() {
    let parsed = parse_input(&format!("  {}  ", BASE32_HASH)).expect("base32 accepted");

    assert!(
        parsed.info_hash.len() == 40 && parsed.info_hash.chars().all(|c| c.is_ascii_hexdigit()),
        "expected 40 hex chars, got {}",
        parsed.info_hash
    );
    assert!(parsed.magnet.contains(&format!("xt=urn:btih:{}", parsed.info_hash)));
}

#[test]
fn test_parse_input_rejects_queries_and_junk() {
    assert_eq!(parse_input("the office 1080p"), None);
    assert_eq!(parse_input(&"g".repeat(40)), None);
    assert_eq!(parse_input("magnet:?xt=urn:btih:tooshort"), None);
    assert_eq!(parse_input(""), None);
}
