use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use rand::RngCore;

const NONCE_LEN: usize = 12;

// from_slice 在长度不符时 panic；主密钥契约是 32 字节（main.rs 已校验），
// 此处为 API 边界防御，把 panic 转为 Err
fn cipher(key: &[u8]) -> Result<Aes256Gcm, String> {
    if key.len() != 32 {
        return Err(format!("master key must be 32 bytes, got {}", key.len()));
    }
    Ok(Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key)))
}

pub fn encrypt_value(key: &[u8], plaintext: &str) -> Result<Vec<u8>, String> {
    let cipher = cipher(key)?;
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn decrypt_value(key: &[u8], blob: &[u8]) -> Result<String, String> {
    if blob.len() < NONCE_LEN + 1 {
        return Err("ciphertext too short".into());
    }
    let (nonce, ct) = blob.split_at(NONCE_LEN);
    let cipher = cipher(key)?;
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|e| e.to_string())?;
    String::from_utf8(pt).map_err(|e| e.to_string())
}
