//! Chaves e assinaturas Ed25519 dos repositórios. Quem assina é o `prancha-ext`, em `extensions/tools`.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Chave pública em base64. Chaves fracas são recusadas: qualquer pessoa forjaria assinaturas para elas.
pub fn parse_public_key(text: &str) -> Option<VerifyingKey> {
    let bytes: [u8; 32] = B64.decode(text.trim()).ok()?.try_into().ok()?;
    VerifyingKey::from_bytes(&bytes).ok().filter(|key| !key.is_weak())
}

pub fn fingerprint(key: &VerifyingKey) -> String {
    sha256_hex(key.as_bytes())
}

// As mensagens têm prefixo próprio: a assinatura de um índice nunca vale como a de um pacote.

fn verify(key: &VerifyingKey, message: &[u8], signature: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(signature) else { return false };
    let Ok(bytes) = B64.decode(text.trim()) else { return false };
    let Ok(signature) = Signature::from_slice(&bytes) else { return false };
    key.verify_strict(message, &signature).is_ok()
}

pub fn verify_index(key: &VerifyingKey, index: &[u8], signature: &[u8]) -> bool {
    verify(key, &[b"prancha-index-v1\0".as_slice(), index].concat(), signature)
}

pub fn verify_package(key: &VerifyingKey, repository: &str, extension: &str, version: &str, sha256: &str, signature: &[u8]) -> bool {
    verify(key, format!("prancha-package-v1\0{repository}\0{extension}\0{version}\0{sha256}").as_bytes(), signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn encoded(signature: Signature) -> Vec<u8> {
        B64.encode(signature.to_bytes()).into_bytes()
    }

    #[test]
    fn an_index_signature_verifies_and_does_not_survive_a_change() {
        let signature = encoded(key().sign(b"prancha-index-v1\0{}"));
        assert!(verify_index(&key().verifying_key(), b"{}", &signature));
        assert!(!verify_index(&key().verifying_key(), b"{ }", &signature));
        let other = SigningKey::from_bytes(&[8u8; 32]);
        assert!(!verify_index(&other.verifying_key(), b"{}", &signature));
    }

    #[test]
    fn a_signature_does_not_cross_domains() {
        let message = "prancha-package-v1\0r\0e\01.0.0\0abc";
        let signature = encoded(key().sign(message.as_bytes()));
        assert!(verify_package(&key().verifying_key(), "r", "e", "1.0.0", "abc", &signature));
        assert!(!verify_package(&key().verifying_key(), "outro", "e", "1.0.0", "abc", &signature));
        assert!(!verify_package(&key().verifying_key(), "r", "e", "1.0.1", "abc", &signature));
        assert!(!verify_index(&key().verifying_key(), message.as_bytes(), &signature));
    }

    #[test]
    fn malformed_keys_and_signatures_are_refused() {
        assert!(parse_public_key("não é base64").is_none());
        assert!(parse_public_key(&B64.encode([1u8; 16])).is_none());
        assert!(parse_public_key(&B64.encode([0u8; 32])).is_none(), "chave fraca");
        assert!(!verify_index(&key().verifying_key(), b"{}", b"lixo"));
        assert!(!verify_index(&key().verifying_key(), b"{}", &[0xff, 0xfe]));
    }

    #[test]
    fn the_fingerprint_is_the_sha256_of_the_raw_key() {
        let public = key().verifying_key();
        let parsed = parse_public_key(&B64.encode(public.as_bytes())).unwrap();
        assert_eq!(fingerprint(&parsed), sha256_hex(public.as_bytes()));
        assert_eq!(fingerprint(&parsed).len(), 64);
    }
}
