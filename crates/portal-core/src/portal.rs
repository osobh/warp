//! Portal lifecycle management
//!
//! This module provides the core Portal type and its lifecycle state machine.
//! Portals represent shared content with strict state transitions and usage tracking.
//!
//! # State Machine
//!
//! Valid state transitions:
//! - Created -> Active (activate)
//! - Active -> Paused (pause)
//! - Active -> Expired (expire)
//! - Paused -> Active (resume)
//! - Paused -> Expired (expire)
//! - Expired -> Archived (archive)
//!
//! # Example
//!
//! ```
//! use portal_core::portal::{Portal, PortalBuilder};
//! use ed25519_dalek::SigningKey;
//! use rand::rngs::OsRng;
//!
//! let signing_key = SigningKey::generate(&mut OsRng);
//! let owner = signing_key.verifying_key();
//!
//! let mut portal = Portal::new("My Portal".to_string(), owner);
//! assert_eq!(portal.state, portal_core::PortalState::Created);
//!
//! portal.activate().expect("Failed to activate");
//! assert!(portal.is_accessible());
//!
//! portal.record_access("accessor-1".to_string(), 1024);
//! assert_eq!(portal.stats.download_count, 1);
//! assert_eq!(portal.stats.bytes_transferred, 1024);
//! ```

use crate::{ContentId, Error, PortalId, PortalState, Result};
use chrono::{DateTime, Utc};
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

/// Portal metadata and state
///
/// A Portal represents shared content with lifecycle management and access tracking.
/// The portal transitions through well-defined states and maintains statistics about
/// access patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Portal {
    /// Unique portal identifier
    pub id: PortalId,
    /// Human-readable portal name
    pub name: String,
    /// Current lifecycle state
    pub state: PortalState,
    /// Portal owner's public key
    pub owner: VerifyingKey,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last modification timestamp
    pub updated_at: DateTime<Utc>,
    /// Root of the content DAG (optional)
    pub content_root: Option<ContentId>,
    /// Access statistics
    pub stats: PortalStats,
}

/// Portal usage statistics
///
/// Tracks download patterns and access metrics for a portal.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PortalStats {
    /// Total number of downloads
    pub download_count: u64,
    /// Total bytes transferred
    pub bytes_transferred: u64,
    /// Set of unique accessor identifiers
    pub unique_accessors: HashSet<String>,
    /// Timestamp of last access
    pub last_accessed: Option<DateTime<Utc>>,
}

impl Portal {
    /// Creates a new portal in the Created state
    ///
    /// # Arguments
    ///
    /// * `name` - Human-readable name for the portal
    /// * `owner` - Public key of the portal owner
    ///
    /// # Returns
    ///
    /// A new Portal in the `Created` state with a randomly generated ID
    #[must_use]
    pub fn new(name: String, owner: VerifyingKey) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name,
            state: PortalState::Created,
            owner,
            created_at: now,
            updated_at: now,
            content_root: None,
            stats: PortalStats::default(),
        }
    }

    /// Activates the portal (Created -> Active)
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidStateTransition` if the portal is not in the Created state
    pub fn activate(&mut self) -> Result<()> {
        self.transition(PortalState::Active)
    }

    /// Pauses the portal (Active -> Paused)
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidStateTransition` if the portal is not in the Active state
    pub fn pause(&mut self) -> Result<()> {
        self.transition(PortalState::Paused)
    }

    /// Resumes the portal (Paused -> Active)
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidStateTransition` if the portal is not in the Paused state
    pub fn resume(&mut self) -> Result<()> {
        self.transition(PortalState::Active)
    }

    /// Expires the portal (Active/Paused -> Expired)
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidStateTransition` if the portal is not in the Active or Paused state
    pub fn expire(&mut self) -> Result<()> {
        self.transition(PortalState::Expired)
    }

    /// Archives the portal (Expired -> Archived)
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidStateTransition` if the portal is not in the Expired state
    pub fn archive(&mut self) -> Result<()> {
        self.transition(PortalState::Archived)
    }

    /// Checks if the portal is accessible for downloads
    ///
    /// # Returns
    ///
    /// `true` if the portal is in the Active state, `false` otherwise
    #[must_use]
    pub fn is_accessible(&self) -> bool {
        self.state == PortalState::Active
    }

    /// Records an access event and updates statistics
    ///
    /// # Arguments
    ///
    /// * `accessor` - Identifier for the entity accessing the portal
    /// * `bytes` - Number of bytes transferred in this access
    pub fn record_access(&mut self, accessor: String, bytes: u64) {
        self.stats.download_count += 1;
        self.stats.bytes_transferred += bytes;
        self.stats.unique_accessors.insert(accessor);
        self.stats.last_accessed = Some(Utc::now());
    }

    /// Sets the content root for this portal
    ///
    /// # Arguments
    ///
    /// * `root` - The content ID of the DAG root
    pub fn set_content_root(&mut self, root: ContentId) {
        self.content_root = Some(root);
        self.updated_at = Utc::now();
    }

    /// Internal state transition with validation
    ///
    /// # Arguments
    ///
    /// * `to` - The target state
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidStateTransition` if the transition is not valid
    fn transition(&mut self, to: PortalState) -> Result<()> {
        // Check if transition is valid
        if !self.is_valid_transition(to) {
            return Err(Error::InvalidStateTransition {
                from: self.state,
                to,
            });
        }

        self.state = to;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Validates whether a state transition is allowed
    ///
    /// # Arguments
    ///
    /// * `to` - The target state
    ///
    /// # Returns
    ///
    /// `true` if the transition is valid, `false` otherwise
    const fn is_valid_transition(&self, to: PortalState) -> bool {
        use PortalState::{Active, Archived, Created, Expired, Paused};

        matches!(
            (self.state, to),
            (Created | Paused, Active)
                | (Active, Paused | Expired)
                | (Paused, Expired)
                | (Expired, Archived)
        )
    }
}

/// Builder pattern for Portal construction
///
/// Provides a fluent API for creating portals with custom configuration.
///
/// # Example
///
/// ```
/// use portal_core::portal::PortalBuilder;
/// use ed25519_dalek::SigningKey;
/// use rand::rngs::OsRng;
/// use uuid::Uuid;
///
/// let signing_key = SigningKey::generate(&mut OsRng);
/// let owner = signing_key.verifying_key();
/// let custom_id = Uuid::new_v4();
///
/// let portal = PortalBuilder::new("My Portal".to_string(), owner)
///     .with_id(custom_id)
///     .build();
///
/// assert_eq!(portal.id, custom_id);
/// assert_eq!(portal.name, "My Portal");
/// ```
#[derive(Debug)]
pub struct PortalBuilder {
    id: Option<PortalId>,
    name: String,
    owner: VerifyingKey,
    content_root: Option<ContentId>,
}

impl PortalBuilder {
    /// Creates a new portal builder
    ///
    /// # Arguments
    ///
    /// * `name` - Human-readable name for the portal
    /// * `owner` - Public key of the portal owner
    #[must_use]
    pub const fn new(name: String, owner: VerifyingKey) -> Self {
        Self {
            id: None,
            name,
            owner,
            content_root: None,
        }
    }

    /// Sets a custom portal ID
    ///
    /// # Arguments
    ///
    /// * `id` - The custom UUID to use for this portal
    #[must_use]
    pub const fn with_id(mut self, id: PortalId) -> Self {
        self.id = Some(id);
        self
    }

    /// Sets the content root for the portal
    ///
    /// # Arguments
    ///
    /// * `root` - The content ID of the DAG root
    #[must_use]
    pub const fn with_content_root(mut self, root: ContentId) -> Self {
        self.content_root = Some(root);
        self
    }

    /// Builds the portal
    ///
    /// # Returns
    ///
    /// A new Portal in the Created state
    #[must_use]
    pub fn build(self) -> Portal {
        let now = Utc::now();
        Portal {
            id: self.id.unwrap_or_else(Uuid::new_v4),
            name: self.name,
            state: PortalState::Created,
            owner: self.owner,
            created_at: now,
            updated_at: now,
            content_root: self.content_root,
            stats: PortalStats::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;
    use std::thread::sleep;
    use std::time::Duration;

    fn create_test_owner() -> VerifyingKey {
        let signing_key = SigningKey::generate(&mut OsRng);
        signing_key.verifying_key()
    }

    fn create_test_content_id() -> ContentId {
        [1u8; 32]
    }

    #[test]
    fn test_portal_creation() {
        let owner = create_test_owner();
        let portal = Portal::new("Test Portal".to_string(), owner);

        assert_eq!(portal.name, "Test Portal");
        assert_eq!(portal.state, PortalState::Created);
        assert_eq!(portal.owner, owner);
        assert!(portal.content_root.is_none());
        assert_eq!(portal.stats.download_count, 0);
        assert_eq!(portal.stats.bytes_transferred, 0);
        assert!(portal.stats.unique_accessors.is_empty());
        assert!(portal.stats.last_accessed.is_none());
    }

    #[test]
    fn test_valid_transitions() {
        let owner = create_test_owner();

        // Test all valid transitions
        let mut portal = Portal::new("Test".to_string(), owner);

        // Created -> Active
        assert!(portal.activate().is_ok());
        assert_eq!(portal.state, PortalState::Active);

        // Active -> Paused
        assert!(portal.pause().is_ok());
        assert_eq!(portal.state, PortalState::Paused);

        // Paused -> Active
        assert!(portal.resume().is_ok());
        assert_eq!(portal.state, PortalState::Active);

        // Active -> Expired
        assert!(portal.expire().is_ok());
        assert_eq!(portal.state, PortalState::Expired);

        // Expired -> Archived
        assert!(portal.archive().is_ok());
        assert_eq!(portal.state, PortalState::Archived);
    }

    #[test]
    fn test_invalid_transitions() {
        let owner = create_test_owner();

        // Test Created -> Paused (invalid)
        let mut portal = Portal::new("Test".to_string(), owner);
        let result = portal.pause();
        assert!(result.is_err());
        if let Err(Error::InvalidStateTransition { from, to }) = result {
            assert_eq!(from, PortalState::Created);
            assert_eq!(to, PortalState::Paused);
        } else {
            panic!("Expected InvalidStateTransition error");
        }

        // Test Created -> Expired (invalid)
        let mut portal = Portal::new("Test".to_string(), owner);
        let result = portal.expire();
        assert!(result.is_err());

        // Test Created -> Archived (invalid)
        let mut portal = Portal::new("Test".to_string(), owner);
        let result = portal.archive();
        assert!(result.is_err());

        // Test Active -> Archived (invalid)
        let mut portal = Portal::new("Test".to_string(), owner);
        portal.activate().unwrap();
        let result = portal.archive();
        assert!(result.is_err());

        // Test Paused -> Archived (invalid)
        let mut portal = Portal::new("Test".to_string(), owner);
        portal.activate().unwrap();
        portal.pause().unwrap();
        let result = portal.archive();
        assert!(result.is_err());

        // Test Expired -> Active (invalid)
        let mut portal = Portal::new("Test".to_string(), owner);
        portal.activate().unwrap();
        portal.expire().unwrap();
        let result = portal.activate();
        assert!(result.is_err());
    }

    #[test]
    fn test_portal_activate() {
        let owner = create_test_owner();
        let mut portal = Portal::new("Test Portal".to_string(), owner);

        assert_eq!(portal.state, PortalState::Created);
        assert!(!portal.is_accessible());

        let result = portal.activate();
        assert!(result.is_ok());
        assert_eq!(portal.state, PortalState::Active);
        assert!(portal.is_accessible());
    }

    #[test]
    fn test_portal_pause_resume() {
        let owner = create_test_owner();
        let mut portal = Portal::new("Test Portal".to_string(), owner);

        // Activate first
        portal.activate().unwrap();
        assert_eq!(portal.state, PortalState::Active);
        assert!(portal.is_accessible());

        // Pause
        let result = portal.pause();
        assert!(result.is_ok());
        assert_eq!(portal.state, PortalState::Paused);
        assert!(!portal.is_accessible());

        // Resume
        let result = portal.resume();
        assert!(result.is_ok());
        assert_eq!(portal.state, PortalState::Active);
        assert!(portal.is_accessible());
    }

    #[test]
    fn test_portal_expire_archive() {
        let owner = create_test_owner();
        let mut portal = Portal::new("Test Portal".to_string(), owner);

        // Activate first
        portal.activate().unwrap();
        assert_eq!(portal.state, PortalState::Active);

        // Expire
        let result = portal.expire();
        assert!(result.is_ok());
        assert_eq!(portal.state, PortalState::Expired);
        assert!(!portal.is_accessible());

        // Archive
        let result = portal.archive();
        assert!(result.is_ok());
        assert_eq!(portal.state, PortalState::Archived);
        assert!(!portal.is_accessible());
    }

    #[test]
    fn test_is_accessible() {
        let owner = create_test_owner();
        let mut portal = Portal::new("Test Portal".to_string(), owner);

        // Created - not accessible
        assert!(!portal.is_accessible());

        // Active - accessible
        portal.activate().unwrap();
        assert!(portal.is_accessible());

        // Paused - not accessible
        portal.pause().unwrap();
        assert!(!portal.is_accessible());

        // Resume to Active - accessible
        portal.resume().unwrap();
        assert!(portal.is_accessible());

        // Expired - not accessible
        portal.expire().unwrap();
        assert!(!portal.is_accessible());

        // Archived - not accessible
        portal.archive().unwrap();
        assert!(!portal.is_accessible());
    }

    #[test]
    fn test_record_access() {
        let owner = create_test_owner();
        let mut portal = Portal::new("Test Portal".to_string(), owner);

        // Record first access
        portal.record_access("accessor-1".to_string(), 1024);
        assert_eq!(portal.stats.download_count, 1);
        assert_eq!(portal.stats.bytes_transferred, 1024);
        assert_eq!(portal.stats.unique_accessors.len(), 1);
        assert!(portal.stats.unique_accessors.contains("accessor-1"));
        assert!(portal.stats.last_accessed.is_some());

        let first_access_time = portal.stats.last_accessed.unwrap();

        // Small delay to ensure timestamp difference
        sleep(Duration::from_millis(10));

        // Record second access from same accessor
        portal.record_access("accessor-1".to_string(), 2048);
        assert_eq!(portal.stats.download_count, 2);
        assert_eq!(portal.stats.bytes_transferred, 3072);
        assert_eq!(portal.stats.unique_accessors.len(), 1);
        assert!(portal.stats.last_accessed.unwrap() > first_access_time);

        // Record access from different accessor
        portal.record_access("accessor-2".to_string(), 512);
        assert_eq!(portal.stats.download_count, 3);
        assert_eq!(portal.stats.bytes_transferred, 3584);
        assert_eq!(portal.stats.unique_accessors.len(), 2);
        assert!(portal.stats.unique_accessors.contains("accessor-1"));
        assert!(portal.stats.unique_accessors.contains("accessor-2"));
    }

    #[test]
    fn test_portal_builder() {
        let owner = create_test_owner();
        let custom_id = Uuid::new_v4();
        let content_root = create_test_content_id();

        // Build with custom ID and content root
        let portal = PortalBuilder::new("Builder Test".to_string(), owner)
            .with_id(custom_id)
            .with_content_root(content_root)
            .build();

        assert_eq!(portal.id, custom_id);
        assert_eq!(portal.name, "Builder Test");
        assert_eq!(portal.state, PortalState::Created);
        assert_eq!(portal.owner, owner);
        assert_eq!(portal.content_root, Some(content_root));

        // Build with defaults
        let portal2 = PortalBuilder::new("Simple Build".to_string(), owner).build();
        assert_eq!(portal2.name, "Simple Build");
        assert!(portal2.content_root.is_none());
    }

    #[test]
    fn test_stats_unique_accessors() {
        let owner = create_test_owner();
        let mut portal = Portal::new("Test Portal".to_string(), owner);

        // Record multiple accesses from same accessor
        portal.record_access("accessor-1".to_string(), 100);
        portal.record_access("accessor-1".to_string(), 200);
        portal.record_access("accessor-1".to_string(), 300);

        // Should have 3 downloads but only 1 unique accessor
        assert_eq!(portal.stats.download_count, 3);
        assert_eq!(portal.stats.bytes_transferred, 600);
        assert_eq!(portal.stats.unique_accessors.len(), 1);

        // Add different accessors
        portal.record_access("accessor-2".to_string(), 100);
        portal.record_access("accessor-3".to_string(), 100);
        portal.record_access("accessor-2".to_string(), 100); // Duplicate

        assert_eq!(portal.stats.download_count, 6);
        assert_eq!(portal.stats.bytes_transferred, 900);
        assert_eq!(portal.stats.unique_accessors.len(), 3);
    }

    #[test]
    fn test_timestamps_updated() {
        let owner = create_test_owner();
        let mut portal = Portal::new("Test Portal".to_string(), owner);

        let created_at = portal.created_at;
        let initial_updated_at = portal.updated_at;

        // Timestamps should be equal at creation
        assert_eq!(created_at, initial_updated_at);

        // Sleep to ensure timestamp difference
        sleep(Duration::from_millis(10));

        // State transition should update updated_at
        portal.activate().unwrap();
        assert_eq!(portal.created_at, created_at); // created_at never changes
        assert!(portal.updated_at > initial_updated_at);

        let after_activate = portal.updated_at;
        sleep(Duration::from_millis(10));

        // Another transition should update again
        portal.pause().unwrap();
        assert_eq!(portal.created_at, created_at);
        assert!(portal.updated_at > after_activate);

        let after_pause = portal.updated_at;
        sleep(Duration::from_millis(10));

        // set_content_root should also update timestamp
        portal.set_content_root(create_test_content_id());
        assert_eq!(portal.created_at, created_at);
        assert!(portal.updated_at > after_pause);
    }

    #[test]
    fn test_set_content_root() {
        let owner = create_test_owner();
        let mut portal = Portal::new("Test Portal".to_string(), owner);

        assert!(portal.content_root.is_none());

        let content_id = create_test_content_id();
        portal.set_content_root(content_id);

        assert_eq!(portal.content_root, Some(content_id));
    }

    #[test]
    fn test_paused_to_expired_transition() {
        let owner = create_test_owner();
        let mut portal = Portal::new("Test Portal".to_string(), owner);

        // Created -> Active -> Paused -> Expired
        portal.activate().unwrap();
        portal.pause().unwrap();
        let result = portal.expire();
        assert!(result.is_ok());
        assert_eq!(portal.state, PortalState::Expired);
    }

    #[test]
    fn test_portal_stats_default() {
        let stats = PortalStats::default();
        assert_eq!(stats.download_count, 0);
        assert_eq!(stats.bytes_transferred, 0);
        assert!(stats.unique_accessors.is_empty());
        assert!(stats.last_accessed.is_none());
    }

    #[test]
    fn test_builder_fluent_api() {
        let owner = create_test_owner();
        let id = Uuid::new_v4();
        let root = create_test_content_id();

        // Test chaining methods
        let portal = PortalBuilder::new("Fluent Test".to_string(), owner)
            .with_id(id)
            .with_content_root(root)
            .build();

        assert_eq!(portal.id, id);
        assert_eq!(portal.content_root, Some(root));
    }

    #[test]
    fn test_multiple_transitions_update_timestamp() {
        let owner = create_test_owner();
        let mut portal = Portal::new("Test Portal".to_string(), owner);

        let mut last_updated = portal.updated_at;

        // Perform multiple transitions and verify timestamp updates
        let transitions = [
            |p: &mut Portal| p.activate(),
            |p: &mut Portal| p.pause(),
            |p: &mut Portal| p.resume(),
            |p: &mut Portal| p.expire(),
            |p: &mut Portal| p.archive(),
        ];

        for transition in &transitions {
            sleep(Duration::from_millis(10));
            transition(&mut portal).unwrap();
            assert!(portal.updated_at > last_updated);
            last_updated = portal.updated_at;
        }
    }

    #[test]
    fn test_record_access_updates_last_accessed() {
        let owner = create_test_owner();
        let mut portal = Portal::new("Test Portal".to_string(), owner);

        assert!(portal.stats.last_accessed.is_none());

        portal.record_access("accessor-1".to_string(), 100);
        let first_access = portal.stats.last_accessed.unwrap();

        sleep(Duration::from_millis(10));

        portal.record_access("accessor-2".to_string(), 200);
        let second_access = portal.stats.last_accessed.unwrap();

        assert!(second_access > first_access);
    }

    #[test]
    fn test_zero_bytes_access() {
        let owner = create_test_owner();
        let mut portal = Portal::new("Test Portal".to_string(), owner);

        portal.record_access("accessor-1".to_string(), 0);
        assert_eq!(portal.stats.download_count, 1);
        assert_eq!(portal.stats.bytes_transferred, 0);
        assert_eq!(portal.stats.unique_accessors.len(), 1);
    }

    #[test]
    fn test_large_bytes_transfer() {
        let owner = create_test_owner();
        let mut portal = Portal::new("Test Portal".to_string(), owner);

        let large_bytes = u64::MAX / 2;
        portal.record_access("accessor-1".to_string(), large_bytes);
        portal.record_access("accessor-2".to_string(), large_bytes);

        assert_eq!(portal.stats.download_count, 2);
        assert_eq!(portal.stats.bytes_transferred, u64::MAX - 1);
    }

    #[test]
    fn test_portal_serialization() {
        let owner = create_test_owner();
        let mut portal = Portal::new("Serialize Test".to_string(), owner);
        portal.activate().unwrap();
        portal.record_access("accessor-1".to_string(), 1024);

        // Test serde serialization
        let serialized = serde_json::to_string(&portal).expect("Failed to serialize");
        let deserialized: Portal =
            serde_json::from_str(&serialized).expect("Failed to deserialize");

        assert_eq!(portal.id, deserialized.id);
        assert_eq!(portal.name, deserialized.name);
        assert_eq!(portal.state, deserialized.state);
        assert_eq!(
            portal.stats.download_count,
            deserialized.stats.download_count
        );
    }
}
