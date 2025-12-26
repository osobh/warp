//! OIDC Identity Provider
//!
//! Supports integration with Keycloak, Auth0, Okta, and other OIDC providers.

use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use tokio::sync::RwLock;

use crate::error::{Error, Result};
use crate::identity::{Group, Identity, IdentityProvider, Principal};
use crate::Credentials;

/// OIDC provider configuration
#[derive(Debug, Clone)]
pub struct OidcConfig {
    /// OIDC issuer URL (e.g., "https://keycloak.example.com/realms/warp")
    pub issuer_url: String,

    /// Client ID
    pub client_id: String,

    /// Client secret
    pub client_secret: String,

    /// Redirect URI for authorization code flow
    pub redirect_uri: Option<String>,

    /// Scopes to request
    pub scopes: Vec<String>,

    /// Claim mappings (OIDC claim -> identity attribute)
    pub claim_mappings: HashMap<String, String>,

    /// Group claim name (default: "groups")
    pub group_claim: String,

    /// Role claim name (default: "roles")
    pub role_claim: String,
}

impl OidcConfig {
    /// Create a new OIDC configuration
    pub fn new(
        issuer_url: impl Into<String>,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
    ) -> Self {
        Self {
            issuer_url: issuer_url.into(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            redirect_uri: None,
            scopes: vec![
                "openid".to_string(),
                "profile".to_string(),
                "email".to_string(),
            ],
            claim_mappings: HashMap::new(),
            group_claim: "groups".to_string(),
            role_claim: "roles".to_string(),
        }
    }

    /// Set redirect URI
    pub fn with_redirect_uri(mut self, uri: impl Into<String>) -> Self {
        self.redirect_uri = Some(uri.into());
        self
    }

    /// Add a scope
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scopes.push(scope.into());
        self
    }

    /// Set group claim name
    pub fn with_group_claim(mut self, claim: impl Into<String>) -> Self {
        self.group_claim = claim.into();
        self
    }

    /// Set role claim name
    pub fn with_role_claim(mut self, claim: impl Into<String>) -> Self {
        self.role_claim = claim.into();
        self
    }

    /// Add a claim mapping
    pub fn with_claim_mapping(
        mut self,
        oidc_claim: impl Into<String>,
        attr: impl Into<String>,
    ) -> Self {
        self.claim_mappings.insert(oidc_claim.into(), attr.into());
        self
    }
}

/// OIDC Discovery Document
#[derive(Debug, Clone, Deserialize)]
struct OidcDiscovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: Option<String>,
    jwks_uri: String,
    #[serde(default)]
    scopes_supported: Vec<String>,
    #[serde(default)]
    response_types_supported: Vec<String>,
    #[serde(default)]
    grant_types_supported: Vec<String>,
}

/// Token response from OIDC provider
#[derive(Debug, Clone, Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    #[serde(default)]
    expires_in: Option<u64>,
    refresh_token: Option<String>,
    id_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

/// UserInfo response from OIDC provider
#[derive(Debug, Clone, Deserialize)]
struct UserInfoResponse {
    sub: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<bool>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    groups: Option<Vec<String>>,
    #[serde(default)]
    roles: Option<Vec<String>>,
    #[serde(flatten)]
    additional_claims: HashMap<String, serde_json::Value>,
}

/// Cached token data
#[derive(Debug, Clone)]
struct CachedToken {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: chrono::DateTime<chrono::Utc>,
}

/// OIDC Identity Provider
pub struct OidcProvider {
    /// Provider ID
    id: String,

    /// Display name
    name: String,

    /// Configuration
    config: OidcConfig,

    /// HTTP client
    http_client: reqwest::Client,

    /// Discovery document (cached)
    discovery: RwLock<Option<OidcDiscovery>>,

    /// Token cache (subject -> cached token)
    token_cache: dashmap::DashMap<String, CachedToken>,
}

impl OidcProvider {
    /// Create a new OIDC provider
    pub fn new(id: impl Into<String>, name: impl Into<String>, config: OidcConfig) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            config,
            http_client: reqwest::Client::new(),
            discovery: RwLock::new(None),
            token_cache: dashmap::DashMap::new(),
        }
    }

    /// Get or fetch the discovery document
    async fn get_discovery(&self) -> Result<OidcDiscovery> {
        // Check cache first
        {
            let guard = self.discovery.read().await;
            if let Some(ref discovery) = *guard {
                return Ok(discovery.clone());
            }
        }

        // Fetch discovery document
        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            self.config.issuer_url.trim_end_matches('/')
        );

        let response = self
            .http_client
            .get(&discovery_url)
            .send()
            .await
            .map_err(|e| Error::Oidc(format!("Failed to fetch discovery: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::Oidc(format!(
                "Discovery request failed: {}",
                response.status()
            )));
        }

        let discovery: OidcDiscovery = response
            .json()
            .await
            .map_err(|e| Error::Oidc(format!("Failed to parse discovery: {}", e)))?;

        // Cache it
        {
            let mut guard = self.discovery.write().await;
            *guard = Some(discovery.clone());
        }

        Ok(discovery)
    }

    /// Exchange authorization code for tokens
    async fn exchange_code(&self, code: &str, redirect_uri: &str) -> Result<TokenResponse> {
        let discovery = self.get_discovery().await?;

        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", &self.config.client_id),
            ("client_secret", &self.config.client_secret),
        ];

        let response = self
            .http_client
            .post(&discovery.token_endpoint)
            .form(&params)
            .send()
            .await
            .map_err(|e| Error::Oidc(format!("Token request failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(Error::Oidc(format!("Token exchange failed: {}", error_text)));
        }

        response
            .json()
            .await
            .map_err(|e| Error::Oidc(format!("Failed to parse token response: {}", e)))
    }

    /// Get user info from access token
    async fn get_userinfo(&self, access_token: &str) -> Result<UserInfoResponse> {
        let discovery = self.get_discovery().await?;

        let userinfo_endpoint = discovery
            .userinfo_endpoint
            .as_ref()
            .ok_or_else(|| Error::Oidc("No userinfo endpoint".to_string()))?;

        let response = self
            .http_client
            .get(userinfo_endpoint)
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| Error::Oidc(format!("UserInfo request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            if status == reqwest::StatusCode::UNAUTHORIZED {
                return Err(Error::InvalidToken("Access token is invalid".to_string()));
            }
            return Err(Error::Oidc(format!("UserInfo request failed: {}", status)));
        }

        response
            .json()
            .await
            .map_err(|e| Error::Oidc(format!("Failed to parse userinfo: {}", e)))
    }

    /// Refresh an access token
    async fn do_refresh_token(&self, refresh_token: &str) -> Result<TokenResponse> {
        let discovery = self.get_discovery().await?;

        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &self.config.client_id),
            ("client_secret", &self.config.client_secret),
        ];

        let response = self
            .http_client
            .post(&discovery.token_endpoint)
            .form(&params)
            .send()
            .await
            .map_err(|e| Error::Oidc(format!("Token refresh request failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(Error::Oidc(format!("Token refresh failed: {}", error_text)));
        }

        response
            .json()
            .await
            .map_err(|e| Error::Oidc(format!("Failed to parse refresh response: {}", e)))
    }

    /// Convert UserInfoResponse to Identity
    fn userinfo_to_identity(&self, userinfo: UserInfoResponse) -> Identity {
        let mut identity = Identity::new(
            userinfo.sub.clone(),
            userinfo
                .name
                .clone()
                .or(userinfo.preferred_username.clone())
                .unwrap_or_else(|| userinfo.sub.clone()),
            &self.id,
        );

        identity.principal = Principal::Federated {
            provider: self.id.clone(),
            subject: userinfo.sub.clone(),
        };

        identity.email = userinfo.email.clone();

        // Extract groups
        if let Some(groups) = userinfo.groups {
            identity.groups = groups;
        } else if let Some(groups_claim) = userinfo.additional_claims.get(&self.config.group_claim)
        {
            if let Some(groups_array) = groups_claim.as_array() {
                identity.groups = groups_array
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
            }
        }

        // Extract roles
        if let Some(roles) = userinfo.roles {
            identity.roles = roles;
        } else if let Some(roles_claim) = userinfo.additional_claims.get(&self.config.role_claim) {
            if let Some(roles_array) = roles_claim.as_array() {
                identity.roles = roles_array
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
            }
        }

        // Apply custom claim mappings
        for (oidc_claim, attr_name) in &self.config.claim_mappings {
            if let Some(value) = userinfo.additional_claims.get(oidc_claim) {
                if let Some(str_value) = value.as_str() {
                    identity
                        .attributes
                        .insert(attr_name.clone(), str_value.to_string());
                }
            }
        }

        // Store all claims
        for (key, value) in userinfo.additional_claims {
            identity.claims.insert(key, value);
        }

        // Add standard claims
        if let Some(ref email) = userinfo.email {
            identity
                .claims
                .insert("email".to_string(), serde_json::json!(email));
        }
        if let Some(ref name) = userinfo.name {
            identity
                .claims
                .insert("name".to_string(), serde_json::json!(name));
        }

        identity
    }

    /// Generate authorization URL for OAuth2 code flow
    pub async fn authorization_url(&self, state: &str) -> Result<String> {
        let discovery = self.get_discovery().await?;

        let redirect_uri = self
            .config
            .redirect_uri
            .as_ref()
            .ok_or_else(|| Error::Oidc("No redirect URI configured".to_string()))?;

        let scopes = self.config.scopes.join(" ");

        let url = format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}",
            discovery.authorization_endpoint,
            urlencoding::encode(&self.config.client_id),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(&scopes),
            urlencoding::encode(state),
        );

        Ok(url)
    }
}

#[async_trait]
impl IdentityProvider for OidcProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn authenticate(&self, credentials: &Credentials) -> Result<Identity> {
        match credentials {
            Credentials::AuthorizationCode { code, redirect_uri } => {
                // Exchange code for tokens
                let token_response = self.exchange_code(code, redirect_uri).await?;

                // Get user info
                let userinfo = self.get_userinfo(&token_response.access_token).await?;

                let identity = self.userinfo_to_identity(userinfo);

                // Cache tokens
                let cached = CachedToken {
                    access_token: token_response.access_token,
                    refresh_token: token_response.refresh_token,
                    expires_at: chrono::Utc::now()
                        + chrono::Duration::seconds(
                            token_response.expires_in.unwrap_or(3600) as i64
                        ),
                };
                self.token_cache.insert(identity.id.clone(), cached);

                Ok(identity)
            }

            Credentials::AccessToken(token) => {
                // Validate access token by calling userinfo endpoint
                self.validate_token(token).await
            }

            _ => Err(Error::AuthenticationFailed(
                "Unsupported credential type for OIDC".to_string(),
            )),
        }
    }

    async fn validate_token(&self, token: &str) -> Result<Identity> {
        let userinfo = self.get_userinfo(token).await?;
        Ok(self.userinfo_to_identity(userinfo))
    }

    async fn refresh_token(&self, refresh_token: &str) -> Result<(String, Option<String>)> {
        let response = self.do_refresh_token(refresh_token).await?;
        Ok((response.access_token, response.refresh_token))
    }

    async fn get_user(&self, user_id: &str) -> Result<Option<Identity>> {
        // Check cache first
        if let Some(cached) = self.token_cache.get(user_id) {
            if cached.expires_at > chrono::Utc::now() {
                // Try to validate the cached access token
                if let Ok(identity) = self.validate_token(&cached.access_token).await {
                    return Ok(Some(identity));
                }
            }
        }

        // Cannot look up users directly - OIDC doesn't support this
        Ok(None)
    }

    async fn get_user_groups(&self, user_id: &str) -> Result<Vec<Group>> {
        // Get identity first
        if let Some(identity) = self.get_user(user_id).await? {
            // Create group objects from identity's group list
            let groups = identity
                .groups
                .iter()
                .map(|g| Group::new(g.clone(), g.clone()))
                .collect();
            return Ok(groups);
        }

        Ok(Vec::new())
    }

    async fn get_group(&self, _group_id: &str) -> Result<Option<Group>> {
        // OIDC doesn't support group lookup
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oidc_config() {
        let config = OidcConfig::new(
            "https://keycloak.example.com/realms/warp",
            "warp-storage",
            "secret",
        )
        .with_redirect_uri("http://localhost:8080/callback")
        .with_scope("offline_access")
        .with_group_claim("realm_groups")
        .with_role_claim("realm_roles")
        .with_claim_mapping("department", "dept");

        assert_eq!(
            config.issuer_url,
            "https://keycloak.example.com/realms/warp"
        );
        assert_eq!(config.client_id, "warp-storage");
        assert_eq!(
            config.redirect_uri,
            Some("http://localhost:8080/callback".to_string())
        );
        assert!(config.scopes.contains(&"offline_access".to_string()));
        assert_eq!(config.group_claim, "realm_groups");
        assert_eq!(config.role_claim, "realm_roles");
        assert_eq!(
            config.claim_mappings.get("department"),
            Some(&"dept".to_string())
        );
    }

    #[test]
    fn test_create_provider() {
        let config = OidcConfig::new(
            "https://keycloak.example.com/realms/warp",
            "warp-storage",
            "secret",
        );

        let provider = OidcProvider::new("keycloak", "Keycloak Provider", config);

        assert_eq!(provider.id(), "keycloak");
        assert_eq!(provider.name(), "Keycloak Provider");
    }

    #[test]
    fn test_userinfo_to_identity() {
        let config = OidcConfig::new("https://keycloak.example.com", "client", "secret");
        let provider = OidcProvider::new("keycloak", "Keycloak", config);

        let userinfo = UserInfoResponse {
            sub: "user-123".to_string(),
            name: Some("Alice".to_string()),
            email: Some("alice@example.com".to_string()),
            email_verified: Some(true),
            preferred_username: Some("alice".to_string()),
            groups: Some(vec!["admins".to_string(), "users".to_string()]),
            roles: Some(vec!["admin".to_string()]),
            additional_claims: HashMap::new(),
        };

        let identity = provider.userinfo_to_identity(userinfo);

        assert_eq!(identity.id, "user-123");
        assert_eq!(identity.name, "Alice");
        assert_eq!(identity.email, Some("alice@example.com".to_string()));
        assert_eq!(identity.groups, vec!["admins", "users"]);
        assert_eq!(identity.roles, vec!["admin"]);
    }
}
