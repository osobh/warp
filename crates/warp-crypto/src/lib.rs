//! warp-crypto: Cryptographic primitives
//!
//! - Encryption: ChaCha20-Poly1305 (AEAD)
//! - Streaming encryption for real-time processing
//! - Signatures: Ed25519
//! - Key exchange: X25519
//! - Key derivation: Argon2id

#![warn(missing_docs)]

pub mod encrypt;
pub mod stream;
pub mod sign;
pub mod kdf;

pub use encrypt::{encrypt, decrypt, Key};
pub use stream::{StreamCipher, StreamCipherBuilder};
pub use sign::{sign, verify, SigningKey, VerifyingKey};
pub use kdf::derive_key;

/// Crypto error types
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Encryption failed
    #[error("Encryption error: {0}")]
    Encryption(String),
    
    /// Decryption failed
    #[error("Decryption error: {0}")]
    Decryption(String),
    
    /// Signature verification failed
    #[error("Signature verification failed")]
    InvalidSignature,
    
    /// Invalid key
    #[error("Invalid key: {0}")]
    InvalidKey(String),
}

/// Result type for crypto operations
pub type Result<T> = std::result::Result<T, Error>;
