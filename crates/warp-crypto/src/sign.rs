//! Ed25519 signatures

pub use ed25519_dalek::{SigningKey, VerifyingKey, Signature};
use ed25519_dalek::Signer;
use crate::{Error, Result};

/// Sign data with Ed25519
pub fn sign(key: &SigningKey, data: &[u8]) -> Signature {
    key.sign(data)
}

/// Verify an Ed25519 signature
pub fn verify(key: &VerifyingKey, data: &[u8], signature: &Signature) -> Result<()> {
    use ed25519_dalek::Verifier;
    key.verify(data, signature)
        .map_err(|_| Error::InvalidSignature)
}

/// Generate a new signing keypair
pub fn generate_keypair() -> SigningKey {
    SigningKey::generate(&mut rand::thread_rng())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sign_verify() {
        let signing_key = generate_keypair();
        let verifying_key = signing_key.verifying_key();
        let data = b"test message";
        
        let signature = sign(&signing_key, data);
        verify(&verifying_key, data, &signature).unwrap();
    }
}
