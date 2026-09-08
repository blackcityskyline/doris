use anyhow::Result;
use std::path::PathBuf;

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 16;

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

pub fn save_credentials(username: &str, password: &str) -> Result<()> {
    let key = aead_key()?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce_bytes)
        .map_err(|e| anyhow::anyhow!("nonce gen error: {}", e))?;
    let nonce = ring::aead::Nonce::assume_unique_for_key(nonce_bytes);

    let mut payload = format!("{}:{}", username, password).into_bytes();
    let aad = ring::aead::Aad::empty();
    key.seal_in_place_append_tag(nonce, aad, &mut payload)
        .map_err(|e| anyhow::anyhow!("seal error: {:?}", e))?;

    let mut output = nonce_bytes.to_vec();
    output.extend_from_slice(&payload);

    let path = credentials_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &output,
    ))?;

    Ok(())
}

pub fn load_credentials() -> Option<(String, String)> {
    let path = credentials_path();
    let data = std::fs::read_to_string(&path).ok()?;
    let decoded = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        data.trim(),
    ).ok()?;

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
    let text = std::str::from_utf8(plaintext).ok()?;

    let mut parts = text.splitn(2, ':');
    let username = parts.next()?.to_string();
    let password = parts.next()?.to_string();

    Some((username, password))
}
