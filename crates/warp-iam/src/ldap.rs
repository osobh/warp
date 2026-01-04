//! LDAP/Active Directory identity provider
//!
//! This module is a placeholder for LDAP integration.
//! Enable with `--features ldap` once ldap3 bindings are implemented.

use crate::{Error, Identity, IdentityProvider, Result};
use async_trait::async_trait;

/// LDAP identity provider configuration
#[derive(Debug, Clone)]
pub struct LdapConfig {
    /// LDAP server URL (e.g., "ldap://localhost:389")
    pub url: String,
    /// Base DN for user searches
    pub base_dn: String,
    /// Bind DN for authentication
    pub bind_dn: Option<String>,
    /// Bind password
    pub bind_password: Option<String>,
}

/// LDAP identity provider
pub struct LdapProvider {
    #[allow(dead_code)]
    config: LdapConfig,
}

impl LdapProvider {
    /// Create a new LDAP provider
    ///
    /// # Errors
    /// Returns error if connection fails
    pub fn new(config: LdapConfig) -> Result<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl IdentityProvider for LdapProvider {
    async fn authenticate(&self, _username: &str, _password: &str) -> Result<Identity> {
        Err(Error::AuthenticationFailed(
            "LDAP provider not yet implemented".to_string(),
        ))
    }

    async fn lookup(&self, _subject: &str) -> Result<Option<Identity>> {
        Err(Error::AuthenticationFailed(
            "LDAP provider not yet implemented".to_string(),
        ))
    }
}
