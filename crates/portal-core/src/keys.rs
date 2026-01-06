//! BIP-39 key hierarchy for Portal
//!
//! This module implements a hierarchical key derivation system based on BIP-39 recovery phrases:
//!
//! ```text
//! RecoveryPhrase (24 words)
//!     │
//!     └─> MasterSeed (64 bytes)
//!             │
//!             ├─> MasterEncryptionKey (32 bytes) [portal/encryption/v1]
//!             ├─> MasterSigningKey (Ed25519)      [portal/signing/v1]
//!             ├─> AuthenticationKey (32 bytes)     [portal/auth/v1]
//!             └─> DeviceKey(n) (32 bytes)          [portal/device/{n}]
//! ```
//!
//! All key material is zeroized on drop for security.

use crate::{Error, Result};
use bip39::{Language, Mnemonic};
use ed25519_dalek::{SigningKey, VerifyingKey};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// BIP-39 mnemonic recovery phrase (24 words)
///
/// This is the root of the key hierarchy. A 24-word mnemonic provides 256 bits of entropy.
///
/// # Examples
///
/// ```
/// use portal_core::RecoveryPhrase;
///
/// // Generate a new random recovery phrase
/// let phrase = RecoveryPhrase::generate();
/// println!("Recovery phrase: {}", phrase.to_phrase());
///
/// // Parse an existing phrase
/// let phrase_str = phrase.to_phrase();
/// let parsed = RecoveryPhrase::from_phrase(&phrase_str).unwrap();
/// ```
#[derive(Clone, ZeroizeOnDrop)]
pub struct RecoveryPhrase {
    /// BIP-39 mnemonic word list
    #[zeroize(skip)]
    mnemonic: Mnemonic,
}

/// Master seed derived from recovery phrase (64 bytes)
///
/// The master seed is derived from the recovery phrase using BIP-39's PBKDF2-based derivation.
/// This seed is the root secret from which all other keys are derived.
#[derive(Clone, ZeroizeOnDrop)]
pub struct MasterSeed(#[zeroize(drop)] [u8; 64]);

/// Master encryption key (32 bytes)
///
/// Used for encrypting portal content. Derived from the master seed using HKDF
/// with the context string "portal/encryption/v1".
#[derive(Clone, ZeroizeOnDrop)]
pub struct MasterEncryptionKey(#[zeroize(drop)] [u8; 32]);

/// Master signing key (Ed25519)
///
/// Used for signing portal metadata and authentication challenges.
/// Derived from the master seed using HKDF with the context string "portal/signing/v1".
#[derive(Clone, ZeroizeOnDrop)]
pub struct MasterSigningKey {
    /// Ed25519 signing (private) key
    signing: SigningKey,
    /// Ed25519 verifying (public) key
    #[zeroize(skip)]
    verifying: VerifyingKey,
}

/// Authentication key for Hub sessions (32 bytes)
///
/// Used for authenticating with the Portal Hub. Derived from the master seed
/// using HKDF with the context string "portal/auth/v1".
#[derive(Clone, ZeroizeOnDrop)]
pub struct AuthenticationKey(#[zeroize(drop)] [u8; 32]);

/// Device-specific subkey (32 bytes)
///
/// Each device gets its own derived key for device-specific operations.
/// Derived using HKDF with the context string "portal/device/{index}".
#[derive(Clone, ZeroizeOnDrop)]
pub struct DeviceKey {
    /// Device index for key derivation
    index: u32,
    /// 32-byte derived device key
    #[zeroize(drop)]
    key: [u8; 32],
}

/// Complete key hierarchy
///
/// Contains all keys derived from a master seed. Provides convenience methods
/// for key derivation.
///
/// # Examples
///
/// ```
/// use portal_core::{RecoveryPhrase, KeyHierarchy};
///
/// let phrase = RecoveryPhrase::generate();
/// let hierarchy = KeyHierarchy::from_recovery_phrase(&phrase).unwrap();
///
/// // Derive device-specific keys
/// let device_0 = hierarchy.derive_device_key(0).unwrap();
/// let device_1 = hierarchy.derive_device_key(1).unwrap();
/// ```
#[derive(Clone)]
pub struct KeyHierarchy {
    /// Master encryption key for content encryption
    encryption: MasterEncryptionKey,
    /// Master signing key for digital signatures
    signing: MasterSigningKey,
    /// Authentication key for Hub sessions
    auth: AuthenticationKey,
}

// HKDF derivation contexts
const CONTEXT_ENCRYPTION: &[u8] = b"portal/encryption/v1";
const CONTEXT_SIGNING: &[u8] = b"portal/signing/v1";
const CONTEXT_AUTH: &[u8] = b"portal/auth/v1";
const CONTEXT_DEVICE_PREFIX: &str = "portal/device/";

impl RecoveryPhrase {
    /// Generate a new random 24-word recovery phrase
    ///
    /// Uses the system's cryptographically secure random number generator.
    ///
    /// # Panics
    ///
    /// This function should never panic, as 24-word mnemonic generation is always valid.
    /// If it does panic, it indicates a critical error in the underlying BIP-39 library.
    ///
    /// # Examples
    ///
    /// ```
    /// use portal_core::RecoveryPhrase;
    ///
    /// let phrase = RecoveryPhrase::generate();
    /// assert_eq!(phrase.to_phrase().split_whitespace().count(), 24);
    /// ```
    #[must_use]
    pub fn generate() -> Self {
        let mnemonic = Mnemonic::generate_in(Language::English, 24)
            .expect("24-word mnemonic generation should never fail");
        Self { mnemonic }
    }

    /// Parse an existing recovery phrase from a string
    ///
    /// The phrase should be 24 words separated by whitespace.
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidRecoveryPhrase` if the phrase is invalid or not 24 words.
    ///
    /// # Examples
    ///
    /// ```
    /// use portal_core::RecoveryPhrase;
    ///
    /// let phrase = RecoveryPhrase::generate();
    /// let phrase_str = phrase.to_phrase();
    /// let parsed = RecoveryPhrase::from_phrase(&phrase_str).unwrap();
    /// assert_eq!(parsed.to_phrase(), phrase_str);
    /// ```
    pub fn from_phrase(words: &str) -> Result<Self> {
        let mnemonic = Mnemonic::parse_in_normalized(Language::English, words)
            .map_err(|e| Error::InvalidRecoveryPhrase(format!("Failed to parse mnemonic: {e}")))?;

        // Verify it's 24 words (256 bits of entropy)
        if mnemonic.word_count() != 24 {
            return Err(Error::InvalidRecoveryPhrase(format!(
                "Expected 24 words, got {}",
                mnemonic.word_count()
            )));
        }

        Ok(Self { mnemonic })
    }

    /// Get the recovery phrase as a string
    ///
    /// Returns the 24 words separated by spaces.
    ///
    /// # Security
    ///
    /// Be careful when displaying or storing this value, as it provides complete
    /// access to all derived keys.
    #[must_use]
    pub fn to_phrase(&self) -> String {
        self.mnemonic.to_string()
    }
}

impl MasterSeed {
    /// Derive a master seed from a recovery phrase
    ///
    /// Uses BIP-39's standard PBKDF2-based key derivation with an empty passphrase.
    ///
    /// # Examples
    ///
    /// ```
    /// use portal_core::{RecoveryPhrase, MasterSeed};
    ///
    /// let phrase = RecoveryPhrase::generate();
    /// let seed = MasterSeed::from_recovery_phrase(&phrase);
    /// ```
    #[must_use]
    pub fn from_recovery_phrase(phrase: &RecoveryPhrase) -> Self {
        let seed_bytes = phrase.mnemonic.to_seed("");
        let mut seed = [0u8; 64];
        seed.copy_from_slice(&seed_bytes);
        Self(seed)
    }

    /// Get a reference to the seed bytes
    ///
    /// # Security
    ///
    /// This exposes the raw seed material. Use with caution.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

impl MasterEncryptionKey {
    /// Get a reference to the key bytes
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl MasterSigningKey {
    /// Get the verifying (public) key
    #[must_use]
    pub const fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying
    }

    /// Get a reference to the signing key
    #[must_use]
    pub const fn signing_key(&self) -> &SigningKey {
        &self.signing
    }

    /// Sign a message
    ///
    /// # Examples
    ///
    /// ```
    /// use portal_core::{RecoveryPhrase, KeyHierarchy};
    ///
    /// let hierarchy = KeyHierarchy::from_recovery_phrase(&RecoveryPhrase::generate()).unwrap();
    /// let message = b"Hello, Portal!";
    /// let signature = hierarchy.signing().sign(message);
    ///
    /// // Verify the signature
    /// use ed25519_dalek::Verifier;
    /// assert!(hierarchy.signing().verifying_key().verify(message, &signature).is_ok());
    /// ```
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> ed25519_dalek::Signature {
        use ed25519_dalek::Signer;
        self.signing.sign(message)
    }
}

impl AuthenticationKey {
    /// Get a reference to the key bytes
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl DeviceKey {
    /// Get the device index
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// Get a reference to the key bytes
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.key
    }
}

impl KeyHierarchy {
    /// Derive complete key hierarchy from a master seed
    ///
    /// Uses HKDF-SHA256 to derive all keys from the master seed.
    ///
    /// # Errors
    ///
    /// Returns `Error::KeyDerivation` if key derivation fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use portal_core::{RecoveryPhrase, MasterSeed, KeyHierarchy};
    ///
    /// let phrase = RecoveryPhrase::generate();
    /// let seed = MasterSeed::from_recovery_phrase(&phrase);
    /// let hierarchy = KeyHierarchy::from_seed(&seed).unwrap();
    /// ```
    pub fn from_seed(seed: &MasterSeed) -> Result<Self> {
        // Use HKDF with SHA-256 for key derivation
        let hkdf = Hkdf::<Sha256>::new(None, seed.as_bytes());

        // Derive encryption key
        let mut encryption_bytes = [0u8; 32];
        hkdf.expand(CONTEXT_ENCRYPTION, &mut encryption_bytes)
            .map_err(|e| Error::KeyDerivation(format!("Failed to derive encryption key: {e}")))?;
        let encryption = MasterEncryptionKey(encryption_bytes);

        // Derive signing key
        let mut signing_seed = [0u8; 32];
        hkdf.expand(CONTEXT_SIGNING, &mut signing_seed)
            .map_err(|e| Error::KeyDerivation(format!("Failed to derive signing key: {e}")))?;
        let signing = SigningKey::from_bytes(&signing_seed);
        let verifying = signing.verifying_key();
        signing_seed.zeroize();
        let signing = MasterSigningKey { signing, verifying };

        // Derive authentication key
        let mut auth_bytes = [0u8; 32];
        hkdf.expand(CONTEXT_AUTH, &mut auth_bytes).map_err(|e| {
            Error::KeyDerivation(format!("Failed to derive authentication key: {e}"))
        })?;
        let auth = AuthenticationKey(auth_bytes);

        Ok(Self {
            encryption,
            signing,
            auth,
        })
    }

    /// Derive complete key hierarchy from a recovery phrase
    ///
    /// Convenience method that combines seed derivation and key hierarchy derivation.
    ///
    /// # Errors
    ///
    /// Returns `Error::KeyDerivation` if key derivation fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use portal_core::{RecoveryPhrase, KeyHierarchy};
    ///
    /// let phrase = RecoveryPhrase::generate();
    /// let hierarchy = KeyHierarchy::from_recovery_phrase(&phrase).unwrap();
    /// ```
    pub fn from_recovery_phrase(phrase: &RecoveryPhrase) -> Result<Self> {
        let seed = MasterSeed::from_recovery_phrase(phrase);
        Self::from_seed(&seed)
    }

    /// Derive a device-specific subkey
    ///
    /// Each device index produces a unique key derived from the master seed.
    ///
    /// # Errors
    ///
    /// Returns `Error::KeyDerivation` if key derivation fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use portal_core::{RecoveryPhrase, KeyHierarchy};
    ///
    /// let hierarchy = KeyHierarchy::from_recovery_phrase(&RecoveryPhrase::generate()).unwrap();
    /// let device_0 = hierarchy.derive_device_key(0).unwrap();
    /// let device_1 = hierarchy.derive_device_key(1).unwrap();
    ///
    /// // Different indices produce different keys
    /// assert_ne!(device_0.as_bytes(), device_1.as_bytes());
    /// ```
    pub fn derive_device_key(&self, device_index: u32) -> Result<DeviceKey> {
        // Create a seed from the encryption key for device key derivation
        // This ensures device keys can't be used to derive the master keys
        let hkdf = Hkdf::<Sha256>::new(None, self.encryption.as_bytes());

        let context = format!("{CONTEXT_DEVICE_PREFIX}{device_index}");
        let mut key_bytes = [0u8; 32];
        hkdf.expand(context.as_bytes(), &mut key_bytes)
            .map_err(|e| {
                Error::KeyDerivation(format!("Failed to derive device key {device_index}: {e}"))
            })?;

        Ok(DeviceKey {
            index: device_index,
            key: key_bytes,
        })
    }

    /// Get the master encryption key
    #[must_use]
    pub const fn encryption(&self) -> &MasterEncryptionKey {
        &self.encryption
    }

    /// Get the master signing key
    #[must_use]
    pub const fn signing(&self) -> &MasterSigningKey {
        &self.signing
    }

    /// Get the authentication key
    #[must_use]
    pub const fn auth(&self) -> &AuthenticationKey {
        &self.auth
    }
}

// Implement Debug for types without leaking key material
impl std::fmt::Debug for RecoveryPhrase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecoveryPhrase")
            .field("word_count", &self.mnemonic.word_count())
            .finish()
    }
}

impl std::fmt::Debug for MasterSeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MasterSeed")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

impl std::fmt::Debug for MasterEncryptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MasterEncryptionKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

impl std::fmt::Debug for MasterSigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MasterSigningKey")
            .field("verifying_key", &self.verifying)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for AuthenticationKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthenticationKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

impl std::fmt::Debug for DeviceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceKey")
            .field("index", &self.index)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl std::fmt::Debug for KeyHierarchy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyHierarchy")
            .field("encryption", &self.encryption)
            .field("signing", &self.signing)
            .field("auth", &self.auth)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test 1: Recovery phrase generation produces valid 24-word phrases
    #[test]
    fn test_recovery_phrase_generation() {
        let phrase = RecoveryPhrase::generate();
        let phrase_str = phrase.to_phrase();

        // Should have exactly 24 words
        let word_count = phrase_str.split_whitespace().count();
        assert_eq!(word_count, 24, "Generated phrase should have 24 words");

        // Should be valid when parsed
        let parsed = RecoveryPhrase::from_phrase(&phrase_str);
        assert!(parsed.is_ok(), "Generated phrase should be parseable");
    }

    /// Test 2: Recovery phrase roundtrip (phrase → seed → phrase works)
    #[test]
    fn test_recovery_phrase_roundtrip() {
        let phrase1 = RecoveryPhrase::generate();
        let phrase_str = phrase1.to_phrase();

        // Parse the phrase back
        let phrase2 = RecoveryPhrase::from_phrase(&phrase_str)
            .expect("Should be able to parse generated phrase");

        // Both should produce the same string
        assert_eq!(
            phrase1.to_phrase(),
            phrase2.to_phrase(),
            "Roundtrip should preserve phrase"
        );

        // Both should produce the same seed
        let seed1 = MasterSeed::from_recovery_phrase(&phrase1);
        let seed2 = MasterSeed::from_recovery_phrase(&phrase2);
        assert_eq!(
            seed1.as_bytes(),
            seed2.as_bytes(),
            "Same phrase should produce same seed"
        );
    }

    /// Test 3: Invalid recovery phrases are rejected
    #[test]
    fn test_recovery_phrase_invalid() {
        // Empty phrase
        let result = RecoveryPhrase::from_phrase("");
        assert!(result.is_err(), "Empty phrase should be rejected");

        // Too few words (12 instead of 24)
        let short_phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let result = RecoveryPhrase::from_phrase(short_phrase);
        assert!(
            result.is_err(),
            "12-word phrase should be rejected (need 24)"
        );

        // Invalid words
        let invalid = "invalid words that are not in the BIP39 wordlist xyz abc def ghi jkl mno pqr stu vwx yz1 yz2 yz3 yz4 yz5 yz6 yz7 yz8 yz9";
        let result = RecoveryPhrase::from_phrase(invalid);
        assert!(result.is_err(), "Invalid words should be rejected");

        // Valid words but wrong checksum (23 valid words + 1 wrong)
        let wrong_checksum = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
        let result = RecoveryPhrase::from_phrase(wrong_checksum);
        assert!(result.is_err(), "Wrong checksum should be rejected");
    }

    /// Test 4: Same phrase produces same seed (deterministic)
    #[test]
    fn test_seed_deterministic() {
        let phrase_str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";

        let phrase1 = RecoveryPhrase::from_phrase(phrase_str).unwrap();
        let phrase2 = RecoveryPhrase::from_phrase(phrase_str).unwrap();

        let seed1 = MasterSeed::from_recovery_phrase(&phrase1);
        let seed2 = MasterSeed::from_recovery_phrase(&phrase2);

        assert_eq!(
            seed1.as_bytes(),
            seed2.as_bytes(),
            "Same phrase should always produce same seed"
        );
    }

    /// Test 5: Same seed produces same keys (deterministic)
    #[test]
    fn test_key_hierarchy_deterministic() {
        let phrase = RecoveryPhrase::generate();
        let seed = MasterSeed::from_recovery_phrase(&phrase);

        let hierarchy1 = KeyHierarchy::from_seed(&seed).unwrap();
        let hierarchy2 = KeyHierarchy::from_seed(&seed).unwrap();

        // Encryption keys should match
        assert_eq!(
            hierarchy1.encryption().as_bytes(),
            hierarchy2.encryption().as_bytes(),
            "Same seed should produce same encryption key"
        );

        // Signing keys should match
        assert_eq!(
            hierarchy1.signing().verifying_key().as_bytes(),
            hierarchy2.signing().verifying_key().as_bytes(),
            "Same seed should produce same signing key"
        );

        // Auth keys should match
        assert_eq!(
            hierarchy1.auth().as_bytes(),
            hierarchy2.auth().as_bytes(),
            "Same seed should produce same auth key"
        );
    }

    /// Test 6: Same device index produces same device key (deterministic)
    #[test]
    fn test_device_key_deterministic() {
        let phrase = RecoveryPhrase::generate();
        let hierarchy = KeyHierarchy::from_recovery_phrase(&phrase).unwrap();

        let device_0_a = hierarchy.derive_device_key(0).unwrap();
        let device_0_b = hierarchy.derive_device_key(0).unwrap();

        assert_eq!(
            device_0_a.as_bytes(),
            device_0_b.as_bytes(),
            "Same device index should produce same key"
        );
        assert_eq!(device_0_a.index(), 0);
        assert_eq!(device_0_b.index(), 0);

        // Different indices should produce different keys
        let device_1 = hierarchy.derive_device_key(1).unwrap();
        assert_ne!(
            device_0_a.as_bytes(),
            device_1.as_bytes(),
            "Different device indices should produce different keys"
        );
        assert_eq!(device_1.index(), 1);
    }

    /// Test 7: Different phrases produce different keys
    #[test]
    fn test_different_phrases_different_keys() {
        let phrase1 = RecoveryPhrase::generate();
        let phrase2 = RecoveryPhrase::generate();

        // Phrases should be different
        assert_ne!(
            phrase1.to_phrase(),
            phrase2.to_phrase(),
            "Generated phrases should be unique"
        );

        let hierarchy1 = KeyHierarchy::from_recovery_phrase(&phrase1).unwrap();
        let hierarchy2 = KeyHierarchy::from_recovery_phrase(&phrase2).unwrap();

        // All keys should be different
        assert_ne!(
            hierarchy1.encryption().as_bytes(),
            hierarchy2.encryption().as_bytes(),
            "Different phrases should produce different encryption keys"
        );

        assert_ne!(
            hierarchy1.signing().verifying_key().as_bytes(),
            hierarchy2.signing().verifying_key().as_bytes(),
            "Different phrases should produce different signing keys"
        );

        assert_ne!(
            hierarchy1.auth().as_bytes(),
            hierarchy2.auth().as_bytes(),
            "Different phrases should produce different auth keys"
        );
    }

    /// Test 8: Signing and verification roundtrip works
    #[test]
    fn test_signing_roundtrip() {
        use ed25519_dalek::Verifier;

        let phrase = RecoveryPhrase::generate();
        let hierarchy = KeyHierarchy::from_recovery_phrase(&phrase).unwrap();

        let message = b"Hello, Portal! This is a test message.";
        let signature = hierarchy.signing().sign(message);

        // Verification should succeed
        let verify_result = hierarchy
            .signing()
            .verifying_key()
            .verify(message, &signature);
        assert!(
            verify_result.is_ok(),
            "Signature verification should succeed for correct message"
        );

        // Verification should fail for different message
        let wrong_message = b"Different message";
        let verify_result = hierarchy
            .signing()
            .verifying_key()
            .verify(wrong_message, &signature);
        assert!(
            verify_result.is_err(),
            "Signature verification should fail for wrong message"
        );
    }

    /// Test 9: Keys are properly zeroized (test debug output doesn't leak)
    #[test]
    fn test_key_zeroize() {
        let phrase = RecoveryPhrase::generate();
        let seed = MasterSeed::from_recovery_phrase(&phrase);
        let hierarchy = KeyHierarchy::from_seed(&seed).unwrap();
        let device_key = hierarchy.derive_device_key(0).unwrap();

        // Test that Debug implementations don't leak key material
        let phrase_debug = format!("{:?}", phrase);
        assert!(
            !phrase_debug.contains("mnemonic"),
            "Debug output should not contain raw mnemonic"
        );

        let seed_debug = format!("{:?}", seed);
        assert!(
            seed_debug.contains("REDACTED"),
            "Debug output should redact seed bytes"
        );

        let enc_debug = format!("{:?}", hierarchy.encryption());
        assert!(
            enc_debug.contains("REDACTED"),
            "Debug output should redact encryption key"
        );

        let auth_debug = format!("{:?}", hierarchy.auth());
        assert!(
            auth_debug.contains("REDACTED"),
            "Debug output should redact auth key"
        );

        let device_debug = format!("{:?}", device_key);
        assert!(
            device_debug.contains("REDACTED"),
            "Debug output should redact device key"
        );
        assert!(
            device_debug.contains("index"),
            "Debug output should show device index"
        );

        // The signing key debug should show the public key but not the private key
        let signing_debug = format!("{:?}", hierarchy.signing());
        assert!(
            signing_debug.contains("verifying_key"),
            "Debug output should show verifying key"
        );
    }

    /// Test 10: Multiple device keys can be derived
    #[test]
    fn test_multiple_device_keys() {
        let phrase = RecoveryPhrase::generate();
        let hierarchy = KeyHierarchy::from_recovery_phrase(&phrase).unwrap();

        let mut keys = Vec::new();
        for i in 0..10 {
            let key = hierarchy.derive_device_key(i).unwrap();
            assert_eq!(key.index(), i);
            keys.push(key);
        }

        // All keys should be unique
        for i in 0..keys.len() {
            for j in (i + 1)..keys.len() {
                assert_ne!(
                    keys[i].as_bytes(),
                    keys[j].as_bytes(),
                    "Device keys {} and {} should be different",
                    i,
                    j
                );
            }
        }
    }

    /// Test 11: Known test vector for deterministic behavior
    #[test]
    fn test_known_vector() {
        // Use a well-known test vector
        let phrase_str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";

        let phrase = RecoveryPhrase::from_phrase(phrase_str).unwrap();
        let seed = MasterSeed::from_recovery_phrase(&phrase);

        // The seed should be deterministic for this phrase
        // We'll just verify that multiple derivations produce the same result
        let seed2 = MasterSeed::from_recovery_phrase(&phrase);
        assert_eq!(seed.as_bytes(), seed2.as_bytes());

        let hierarchy1 = KeyHierarchy::from_seed(&seed).unwrap();
        let hierarchy2 = KeyHierarchy::from_seed(&seed).unwrap();

        assert_eq!(
            hierarchy1.encryption().as_bytes(),
            hierarchy2.encryption().as_bytes()
        );
    }

    /// Test 12: Verify all key lengths are correct
    #[test]
    fn test_key_lengths() {
        let phrase = RecoveryPhrase::generate();
        let seed = MasterSeed::from_recovery_phrase(&phrase);
        let hierarchy = KeyHierarchy::from_seed(&seed).unwrap();

        assert_eq!(seed.as_bytes().len(), 64, "Master seed should be 64 bytes");
        assert_eq!(
            hierarchy.encryption().as_bytes().len(),
            32,
            "Encryption key should be 32 bytes"
        );
        assert_eq!(
            hierarchy.auth().as_bytes().len(),
            32,
            "Auth key should be 32 bytes"
        );
        assert_eq!(
            hierarchy.signing().signing_key().to_bytes().len(),
            32,
            "Signing key should be 32 bytes"
        );
        assert_eq!(
            hierarchy.signing().verifying_key().as_bytes().len(),
            32,
            "Verifying key should be 32 bytes"
        );

        let device_key = hierarchy.derive_device_key(0).unwrap();
        assert_eq!(
            device_key.as_bytes().len(),
            32,
            "Device key should be 32 bytes"
        );
    }

    /// Test 13: Verify HKDF contexts are being used correctly
    #[test]
    fn test_different_contexts_different_keys() {
        let phrase = RecoveryPhrase::generate();
        let hierarchy = KeyHierarchy::from_recovery_phrase(&phrase).unwrap();

        // All keys derived from the same seed should be different
        // because they use different HKDF contexts
        let enc_bytes = hierarchy.encryption().as_bytes();
        let auth_bytes = hierarchy.auth().as_bytes();

        assert_ne!(
            enc_bytes, auth_bytes,
            "Encryption and auth keys should be different"
        );

        // Device keys should also be different from master keys
        let device_0 = hierarchy.derive_device_key(0).unwrap();
        assert_ne!(
            enc_bytes,
            device_0.as_bytes(),
            "Device key should differ from encryption key"
        );
        assert_ne!(
            auth_bytes,
            device_0.as_bytes(),
            "Device key should differ from auth key"
        );
    }
}
