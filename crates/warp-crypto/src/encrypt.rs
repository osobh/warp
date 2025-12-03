//! ChaCha20-Poly1305 AEAD encryption

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::{rngs::OsRng, RngCore};
use zeroize::Zeroize;

use crate::{Error, Result};

/// Encryption key (32 bytes)
#[derive(Clone, Zeroize)]
#[zeroize(drop)]
pub struct Key([u8; 32]);

impl Key {
    /// Create from bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    
    /// Generate random key
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }
    
    /// Get bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Encrypt data with ChaCha20-Poly1305
///
/// Returns: nonce (12 bytes) || ciphertext || tag (16 bytes)
pub fn encrypt(key: &Key, plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(key.0.as_ref().into());
    
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| Error::Encryption(e.to_string()))?;
    
    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    
    Ok(result)
}

/// Decrypt data with ChaCha20-Poly1305
///
/// Input: nonce (12 bytes) || ciphertext || tag (16 bytes)
pub fn decrypt(key: &Key, ciphertext: &[u8]) -> Result<Vec<u8>> {
    if ciphertext.len() < 12 + 16 {
        return Err(Error::Decryption("Ciphertext too short".into()));
    }
    
    let cipher = ChaCha20Poly1305::new(key.0.as_ref().into());
    let nonce = Nonce::from_slice(&ciphertext[..12]);
    
    cipher
        .decrypt(nonce, &ciphertext[12..])
        .map_err(|e| Error::Decryption(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_roundtrip() {
        let key = Key::generate();
        let plaintext = b"hello, world!";
        
        let ciphertext = encrypt(&key, plaintext).unwrap();
        let decrypted = decrypt(&key, &ciphertext).unwrap();
        
        assert_eq!(plaintext.as_slice(), decrypted.as_slice());
    }
}
