//! Encrypted credential storage.
//!
//! Storage is keyed by resource id (`"rutracker"`, later `"rutor"`, ...) so
//! the multi-tab Login modal (ROADMAP.md Phase 6) can hold one saved login
//! per source without them clobbering each other. The payload is JSON
//! before encryption, not a hand-rolled `"user:pass"` string — the old
//! colon-joined format silently corrupted any password containing `:`.
//!
//! Reading an old (pre-keyed-store) credentials file still works: the
//! legacy `"user:pass"` payload is recognised and transparently treated as
//! the `rutracker` entry, so nobody has to re-enter a saved login just
//! because of this change.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 16;

/// Resource id used by the single-resource compatibility wrappers
/// (`save_credentials`/`load_credentials`) at the bottom of this file, and
/// by the legacy-format migration. Matches `Source::id()` for Rutracker,
/// the only source that exists today.
const DEFAULT_RESOURCE: &str = "rutracker";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub username: String,
    pub password: String,
}

pub fn credentials_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("doris").join("credentials.enc")
}

fn derive_key() -> Result<[u8; KEY_LEN]> {
    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "default".to_string());
    let username = whoami::username();

    let mut input = Vec::new();
    input.extend_from_slice(hostname.as_bytes());
    input.extend_from_slice(username.as_bytes());
    input.extend_from_slice(b"doris-cred-salt-v1");

    let hash = ring::digest::digest(&ring::digest::SHA256, &input);
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&hash.as_ref()[..KEY_LEN]);
    Ok(key)
}

fn aead_key() -> Result<ring::aead::LessSafeKey> {
    let key_bytes = derive_key()?;
    let unbound = ring::aead::UnboundKey::new(&ring::aead::AES_128_GCM, &key_bytes)
        .map_err(|e| anyhow::anyhow!("key error: {:?}", e))?;
    Ok(ring::aead::LessSafeKey::new(unbound))
}

fn decrypt_file() -> Option<Vec<u8>> {
    let path = credentials_path();
    let data = std::fs::read_to_string(&path).ok()?;
    let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data.trim()).ok()?;

    if decoded.len() < NONCE_LEN + 16 {
        return None;
    }

    let nonce_bytes = &decoded[..NONCE_LEN];
    let ciphertext = &decoded[NONCE_LEN..];

    let key = aead_key().ok()?;
    let nonce_arr: [u8; NONCE_LEN] = nonce_bytes.try_into().ok()?;
    let nonce = ring::aead::Nonce::assume_unique_for_key(nonce_arr);
    let aad = ring::aead::Aad::empty();

    let mut in_out = ciphertext.to_vec();
    let plaintext = key.open_in_place(nonce, aad, &mut in_out).ok()?;
    Some(plaintext.to_vec())
}

fn encrypt_and_write(plaintext: &[u8]) -> Result<()> {
    let key = aead_key()?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce_bytes).map_err(|e| anyhow::anyhow!("nonce gen error: {}", e))?;
    let nonce = ring::aead::Nonce::assume_unique_for_key(nonce_bytes);

    let mut payload = plaintext.to_vec();
    let aad = ring::aead::Aad::empty();
    key.seal_in_place_append_tag(nonce, aad, &mut payload)
        .map_err(|e| anyhow::anyhow!("seal error: {:?}", e))?;

    let mut output = nonce_bytes.to_vec();
    output.extend_from_slice(&payload);

    let path = credentials_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &output))?;
    Ok(())
}

/// Load the full keyed credential store (resource id -> Credential).
/// Returns an empty map if the file doesn't exist, can't be decrypted
/// (e.g. the machine identity used to derive the key changed), or is
/// corrupt. Transparently upgrades the pre-Phase-4 `"user:pass"` format
/// into `{ DEFAULT_RESOURCE: { username, password } }` on read.
pub fn load_store() -> HashMap<String, Credential> {
    let Some(plaintext) = decrypt_file() else {
        return HashMap::new();
    };

    if let Ok(map) = serde_json::from_slice::<HashMap<String, Credential>>(&plaintext) {
        return map;
    }

    // Legacy fallback: a bare "username:password" payload.
    if let Ok(text) = std::str::from_utf8(&plaintext) {
        let mut parts = text.splitn(2, ':');
        if let (Some(username), Some(password)) = (parts.next(), parts.next()) {
            let mut map = HashMap::new();
            map.insert(
                DEFAULT_RESOURCE.to_string(),
                Credential { username: username.to_string(), password: password.to_string() },
            );
            return map;
        }
    }

    HashMap::new()
}

fn save_store(store: &HashMap<String, Credential>) -> Result<()> {
    let payload = serde_json::to_vec(store)?;
    encrypt_and_write(&payload)
}

/// Save (or overwrite) the credential for one resource, without disturbing
/// any other resource's saved login.
pub fn save_credential(resource_id: &str, username: &str, password: &str) -> Result<()> {
    let mut store = load_store();
    store.insert(
        resource_id.to_string(),
        Credential { username: username.to_string(), password: password.to_string() },
    );
    save_store(&store)
}

pub fn load_credential(resource_id: &str) -> Option<(String, String)> {
    load_store().get(resource_id).map(|c| (c.username.clone(), c.password.clone()))
}

pub fn delete_credential(resource_id: &str) -> Result<()> {
    let mut store = load_store();
    store.remove(resource_id);
    save_store(&store)
}

// --- Backward-compatible single-resource API -------------------------------
// Existing call sites (login modal, health check) predate the multi-source
// Login panel (ROADMAP.md Phase 6) and only ever deal with Rutracker. These
// wrappers keep them working unchanged.

pub fn save_credentials(username: &str, password: &str) -> Result<()> {
    save_credential(DEFAULT_RESOURCE, username, password)
}

pub fn load_credentials() -> Option<(String, String)> {
    load_credential(DEFAULT_RESOURCE)
}
