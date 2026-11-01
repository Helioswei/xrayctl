//! 凭证生成：REALITY 的 X25519 密钥对、UUID、shortId。
//!
//! 关键点：能纯本地算，就不必 ssh 到服务器上跑 `xray uuid` / `xray x25519`。
//! 编码格式必须和 xray 一致：**base64 url-safe、无 padding**（Go 的 base64.RawURLEncoding）。

use anyhow::{bail, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use x25519_dalek::{PublicKey, StaticSecret};

pub struct RealityKeys {
    pub private: String,
    pub public: String,
}

pub fn generate_keys() -> RealityKeys {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let secret = StaticSecret::from(bytes);
    let public = PublicKey::from(&secret);
    RealityKeys {
        private: URL_SAFE_NO_PAD.encode(secret.to_bytes()),
        public: URL_SAFE_NO_PAD.encode(public.as_bytes()),
    }
}

/// 从私钥推公钥（xray x25519 -i <privateKey> 的等价实现）
pub fn public_from_private(private_b64: &str) -> Result<String> {
    let raw = match URL_SAFE_NO_PAD.decode(private_b64.trim()) {
        Ok(v) => v,
        Err(_) => bail!(
            "私钥不是合法的 base64url：{}",
            &private_b64[..private_b64.len().min(12)]
        ),
    };
    if raw.len() != 32 {
        bail!("私钥长度应为 32 字节，实际 {}", raw.len());
    }
    let mut buf = [0u8; 32];
    buf.copy_from_slice(&raw);
    let secret = StaticSecret::from(buf);
    Ok(URL_SAFE_NO_PAD.encode(PublicKey::from(&secret).as_bytes()))
}

pub fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// shortId：8 字节 hex（16 字符），REALITY 接受偶数长度、最长 16 字符
pub fn new_short_id() -> String {
    let mut b = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_roundtrip() {
        let k = generate_keys();
        assert_eq!(public_from_private(&k.private).unwrap(), k.public);
        assert_eq!(URL_SAFE_NO_PAD.decode(&k.private).unwrap().len(), 32);
    }

    #[test]
    fn short_id_is_16_hex() {
        let s = new_short_id();
        assert_eq!(s.len(), 16);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
