//! 客户端 API key 的静态加密（AES-256-GCM）。
//!
//! 存储格式：base64(nonce(12B) ‖ ciphertext+tag)。
//! 无 key 时退化为明文；解密失败（旧明文 / key 不匹配）原样返回，
//! 以便旧数据平滑过渡。

use aes_gcm::{
    aead::{rand_core::OsRng, Aead, KeyInit},
    Aes256Gcm, AeadCore, Key, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};

const NONCE_LEN: usize = 12;

/// 用配置密钥封装明文：有 key 则加密，否则原样返回。
pub fn seal(key: Option<&[u8; 32]>, plaintext: &str) -> String {
    match key {
        Some(k) => encrypt(k, plaintext).unwrap_or_else(|| plaintext.to_string()),
        None => plaintext.to_string(),
    }
}

/// 还原明文：有 key 则尝试解密，失败或无 key 时原样返回 stored（兼容旧明文）。
pub fn reveal(key: Option<&[u8; 32]>, stored: &str) -> String {
    match key {
        Some(k) => decrypt(k, stored).unwrap_or_else(|| stored.to_string()),
        None => stored.to_string(),
    }
}

/// 解析配置中的 base64(32B) 密钥；格式非法返回 None。
pub fn parse_key(raw: &str) -> Option<[u8; 32]> {
    let bytes = B64.decode(raw.trim()).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Some(arr)
}

fn encrypt(key: &[u8; 32], plaintext: &str) -> Option<String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ct = cipher.encrypt(&nonce, plaintext.as_bytes()).ok()?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(nonce.as_slice());
    out.extend_from_slice(&ct);
    Some(B64.encode(&out))
}

fn decrypt(key: &[u8; 32], blob: &str) -> Option<String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let raw = B64.decode(blob).ok()?;
    if raw.len() <= NONCE_LEN {
        return None;
    }
    let nonce = Nonce::from_slice(&raw[..NONCE_LEN]);
    let pt = cipher.decrypt(nonce, &raw[NONCE_LEN..]).ok()?;
    String::from_utf8(pt).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_key() -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, b) in k.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7).wrapping_add(11);
        }
        k
    }

    #[test]
    fn seal_reveal_roundtrip() {
        let key = fixed_key();
        let plaintext = "mb-12345678-1234-1234-1234-1234567890ab";
        let sealed = seal(Some(&key), plaintext);
        assert_ne!(sealed, plaintext, "sealed output must differ from plaintext");
        assert_eq!(reveal(Some(&key), &sealed), plaintext);
    }

    #[test]
    fn no_key_is_passthrough() {
        // 未配置 key：退化为明文（与旧行为一致）
        let plaintext = "mb-abc";
        assert_eq!(seal(None, plaintext), plaintext);
        assert_eq!(reveal(None, plaintext), plaintext);
    }

    #[test]
    fn reveal_falls_back_on_legacy_plaintext() {
        // 旧明文（非密文）：解密失败时原样返回，平滑兼容历史数据
        let key = fixed_key();
        let legacy = "mb-legacy-unencrypted-key";
        assert_eq!(reveal(Some(&key), legacy), legacy);
    }

    #[test]
    fn parse_key_validates_length() {
        // 非 base64 或长度非 32 字节均拒绝
        assert!(parse_key("not-valid-base64!!!").is_none());
        assert!(parse_key("").is_none());
        // 16 字节的合法 base64 也应被拒（需要恰好 32 字节）
        let short = B64.encode([0u8; 16]);
        assert!(parse_key(&short).is_none());
        let exact = B64.encode([0u8; 32]);
        assert!(parse_key(&exact).is_some());
    }
}
