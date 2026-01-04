//! Key derivation using Argon2id

use argon2::{Argon2, password_hash::SaltString};
use rand::rngs::OsRng;
use zeroize::Zeroize;

use crate::{Error, Result};

/// Derived key (32 bytes)
#[derive(Clone, Zeroize)]
#[zeroize(drop)]
pub struct DerivedKey([u8; 32]);

impl DerivedKey {
    /// Get bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Derive a key from a password using Argon2id
pub fn derive_key(password: &[u8], salt: &[u8]) -> Result<DerivedKey> {
    let mut output = [0u8; 32];

    Argon2::default()
        .hash_password_into(password, salt, &mut output)
        .map_err(|e| Error::InvalidKey(e.to_string()))?;

    Ok(DerivedKey(output))
}

/// Generate a random salt
pub fn generate_salt() -> [u8; 16] {
    let salt = SaltString::generate(&mut OsRng);
    let mut output = [0u8; 16];
    output.copy_from_slice(&salt.as_str().as_bytes()[..16]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive() {
        let password = b"test password";
        let salt = generate_salt();

        let key1 = derive_key(password, &salt).unwrap();
        let key2 = derive_key(password, &salt).unwrap();

        assert_eq!(key1.0, key2.0);
    }
}
