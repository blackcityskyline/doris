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
//!
//! # What the encryption does and does not buy
//!
//! The key is `SHA256(hostname + username + "doris-cred-salt-v1")` -- both
//! inputs are readable by anyone who can read this file (`/etc/hostname`,
//! `whoami`), so **this does not protect against a local reader running as
//! your user**: such a process derives the same key and decrypts the store.
//! Verified by doing exactly that against a real store.
//!
//! What it does buy: the file is not a plaintext password sitting in a
//! config directory (so a stray `cat`, a grep, a crash reporter, a backup
//! that gets attached to a bug report, or an editor that autosaves it does
//! not hand the password over), and the file is 0600, which does stop
//! every *other* account on the machine -- the realistic case for a desktop
//! with more than one user.
//!
//! Protecting against a process running as you would need a key you never
//! write down (a passphrase, or libsecret), which is a UX cost, so it is
//! not done here. That is a deliberate boundary, not an oversight.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 16;

/// Resource id used by the single-resource compatibility wrappers
/// (`save_credentials`/`load_credentials`) at the bottom of this file, and
/// by the legacy-format migration. Matches `Source::id()` for Rutracker,
/// the only source that exists today.
const DEFAULT_RESOURCE: &str = "rutracker";

/// The resource ids the login modal manages, in tab order. Today that is
/// rutracker alone -- the store is keyed by id, so a future source with
/// a session of its own adds its tab here and the modal picks it up
/// without a UI change.
pub const LOGIN_RESOURCES: &[&str] = &[DEFAULT_RESOURCE];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub username: String,
    pub password: String,
}

/// Where the app keeps its credential store.
pub fn credentials_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join("doris").join("credentials.enc")
}

/// The store's file name inside whatever directory it is kept in. The
/// `_at` functions take a directory rather than a file so a caller (a
/// test, or a second store) supplies a location without repeating the
/// name -- and, more to the point, so a test can point the whole store
/// at a scratch directory. The store used to resolve `$HOME` from inside
/// its own I/O, and every test that saved a fake login overwrote the
/// real one.
pub const STORE_FILE: &str = "credentials.enc";

/// Write a file only its owner can read or write, and repair the mode of
/// one that already exists.
///
/// Both files doris keeps that hold a live login -- this store and the
/// saved rutracker cookies, where `bb_session` *is* the session -- go
/// through here, so "this file is secret" is one rule in one place
/// instead of a `fs::write` each somebody can forget. `fs::write` creates
/// 0644 under the default umask 022, which makes both readable by every
/// account on the machine.
///
/// The `mode` on `OpenOptions` only applies when it *creates* the file, so
/// a store an older build already wrote at 0644 would keep that mode
/// forever; the explicit `set_permissions` is what actually repairs it.
#[cfg(unix)]
pub fn write_private(path: &Path, contents: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(contents)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

/// No file modes to set off unix.
#[cfg(not(unix))]
pub fn write_private(path: &Path, contents: &[u8]) -> Result<()> {
    std::fs::write(path, contents)?;
    Ok(())
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

fn decrypt_file(path: &Path) -> Option<Vec<u8>> {
    let data = std::fs::read_to_string(path).ok()?;
    let decoded =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data.trim()).ok()?;

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

fn encrypt_and_write(path: &Path, plaintext: &[u8]) -> Result<()> {
    let key = aead_key()?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce_bytes)
        .map_err(|e| anyhow::anyhow!("nonce gen error: {}", e))?;
    let nonce = ring::aead::Nonce::assume_unique_for_key(nonce_bytes);

    let mut payload = plaintext.to_vec();
    let aad = ring::aead::Aad::empty();
    key.seal_in_place_append_tag(nonce, aad, &mut payload)
        .map_err(|e| anyhow::anyhow!("seal error: {:?}", e))?;

    let mut output = nonce_bytes.to_vec();
    output.extend_from_slice(&payload);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_private(
        path,
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &output).as_bytes(),
    )?;
    Ok(())
}

/// Load the full keyed credential store (resource id -> Credential).
/// Returns an empty map if the file doesn't exist, can't be decrypted
/// (e.g. the machine identity used to derive the key changed), or is
/// corrupt. Transparently upgrades the pre-Phase-4 `"user:pass"` format
/// into `{ DEFAULT_RESOURCE: { username, password } }` on read.
pub fn load_store() -> HashMap<String, Credential> {
    load_store_at(&credentials_path())
}

/// [`load_store`] against an explicit store file.
pub fn load_store_at(path: &Path) -> HashMap<String, Credential> {
    let Some(plaintext) = decrypt_file(path) else {
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
                Credential {
                    username: username.to_string(),
                    password: password.to_string(),
                },
            );
            return map;
        }
    }

    HashMap::new()
}

fn save_store(path: &Path, store: &HashMap<String, Credential>) -> Result<()> {
    let payload = serde_json::to_vec(store)?;
    encrypt_and_write(path, &payload)
}

/// Save (or overwrite) the credential for one resource, without disturbing
/// any other resource's saved login.
pub fn save_credential(resource_id: &str, username: &str, password: &str) -> Result<()> {
    save_credential_at(&credentials_path(), resource_id, username, password)
}

/// [`save_credential`] against an explicit store file.
pub fn save_credential_at(
    path: &Path,
    resource_id: &str,
    username: &str,
    password: &str,
) -> Result<()> {
    let mut store = load_store_at(path);
    store.insert(
        resource_id.to_string(),
        Credential {
            username: username.to_string(),
            password: password.to_string(),
        },
    );
    save_store(path, &store)
}

pub fn load_credential(resource_id: &str) -> Option<(String, String)> {
    load_credential_at(&credentials_path(), resource_id)
}

/// [`load_credential`] against an explicit store file.
pub fn load_credential_at(path: &Path, resource_id: &str) -> Option<(String, String)> {
    load_store_at(path)
        .get(resource_id)
        .map(|c| (c.username.clone(), c.password.clone()))
}

pub fn delete_credential(resource_id: &str) -> Result<()> {
    delete_credential_at(&credentials_path(), resource_id)
}

/// [`delete_credential`] against an explicit store file.
pub fn delete_credential_at(path: &Path, resource_id: &str) -> Result<()> {
    let mut store = load_store_at(path);
    store.remove(resource_id);
    save_store(path, &store)
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
