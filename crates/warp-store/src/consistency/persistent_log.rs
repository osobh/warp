//! Persistent log storage for Raft using sled
//!
//! Provides durable storage for Raft log entries, vote, and committed state.
//! Uses sled embedded database for persistence across restarts.

use std::fmt::Debug;
use std::ops::RangeBounds;
use std::path::Path;
use std::sync::Arc;

use openraft::{
    Entry, LogId, LogState, StorageError, StorageIOError, Vote,
    storage::{RaftLogReader, RaftLogStorage},
};
use sled::Db;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::types::*;

/// Tree names for sled
const TREE_LOG: &str = "raft_log";
const TREE_META: &str = "raft_meta";

/// Keys for metadata tree
const KEY_VOTE: &[u8] = b"vote";
const KEY_COMMITTED: &[u8] = b"committed";
const KEY_LAST_PURGED: &[u8] = b"last_purged";

/// Persistent log store using sled
///
/// Stores Raft log entries durably on disk. Suitable for production use
/// where data must survive restarts.
pub struct SledLogStore {
    /// Sled database handle
    db: Arc<Db>,

    /// Log entries tree
    log_tree: sled::Tree,

    /// Metadata tree (vote, committed, etc.)
    meta_tree: sled::Tree,
}

impl SledLogStore {
    /// Open or create a persistent log store at the given path
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, sled::Error> {
        let db = sled::open(path.as_ref())?;
        Self::from_db(Arc::new(db))
    }

    /// Create from an existing sled database
    pub fn from_db(db: Arc<Db>) -> Result<Self, sled::Error> {
        let log_tree = db.open_tree(TREE_LOG)?;
        let meta_tree = db.open_tree(TREE_META)?;

        info!(
            log_entries = log_tree.len(),
            "Opened persistent Raft log store"
        );

        Ok(Self {
            db,
            log_tree,
            meta_tree,
        })
    }

    /// Get the underlying sled database
    pub fn db(&self) -> &Arc<Db> {
        &self.db
    }

    /// Flush all pending writes to disk
    pub fn flush(&self) -> Result<(), sled::Error> {
        self.log_tree.flush()?;
        self.meta_tree.flush()?;
        Ok(())
    }

    /// Get the last log ID in the store
    fn last_log_id(&self) -> Option<LogId<NodeId>> {
        self.log_tree.last().ok().flatten().and_then(|(_, value)| {
            rmp_serde::from_slice::<Entry<TypeConfig>>(&value)
                .ok()
                .map(|e| e.log_id)
        })
    }

    /// Get the last purged log ID
    fn get_last_purged(&self) -> Option<LogId<NodeId>> {
        self.meta_tree
            .get(KEY_LAST_PURGED)
            .ok()
            .flatten()
            .and_then(|v| rmp_serde::from_slice(&v).ok())
    }

    /// Set the last purged log ID
    fn set_last_purged(&self, log_id: LogId<NodeId>) -> Result<(), sled::Error> {
        let bytes = rmp_serde::to_vec(&log_id).expect("Failed to serialize log_id");
        self.meta_tree.insert(KEY_LAST_PURGED, bytes)?;
        Ok(())
    }

    /// Log index to key bytes (big-endian for proper ordering)
    fn index_to_key(index: u64) -> [u8; 8] {
        index.to_be_bytes()
    }

    /// Key bytes to log index
    fn key_to_index(key: &[u8]) -> u64 {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(key);
        u64::from_be_bytes(bytes)
    }
}

impl RaftLogReader<TypeConfig> for RwLock<SledLogStore> {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry<TypeConfig>>, StorageError<NodeId>> {
        let store = self.read().await;

        // Convert range to key range
        let start = match range.start_bound() {
            std::ops::Bound::Included(&i) => {
                std::ops::Bound::Included(SledLogStore::index_to_key(i))
            }
            std::ops::Bound::Excluded(&i) => {
                std::ops::Bound::Excluded(SledLogStore::index_to_key(i))
            }
            std::ops::Bound::Unbounded => std::ops::Bound::Unbounded,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(&i) => {
                std::ops::Bound::Included(SledLogStore::index_to_key(i))
            }
            std::ops::Bound::Excluded(&i) => {
                std::ops::Bound::Excluded(SledLogStore::index_to_key(i))
            }
            std::ops::Bound::Unbounded => std::ops::Bound::Unbounded,
        };

        let entries: Vec<Entry<TypeConfig>> = store
            .log_tree
            .range((start, end))
            .filter_map(|result| {
                result
                    .ok()
                    .and_then(|(_, value)| rmp_serde::from_slice(&value).ok())
            })
            .collect();

        Ok(entries)
    }
}

impl RaftLogStorage<TypeConfig> for RwLock<SledLogStore> {
    type LogReader = Self;

    async fn read_vote(&mut self) -> Result<Option<Vote<NodeId>>, StorageError<NodeId>> {
        let store = self.read().await;

        let vote = store
            .meta_tree
            .get(KEY_VOTE)
            .map_err(|e| StorageIOError::read_vote(&e))?
            .and_then(|v| rmp_serde::from_slice(&v).ok());

        Ok(vote)
    }

    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, StorageError<NodeId>> {
        let store = self.read().await;

        let last_purged = store.get_last_purged();
        let last = store.last_log_id();

        Ok(LogState {
            last_purged_log_id: last_purged,
            last_log_id: last,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        panic!("get_log_reader called - use try_get_log_entries directly")
    }

    async fn save_vote(&mut self, vote: &Vote<NodeId>) -> Result<(), StorageError<NodeId>> {
        let store = self.write().await;

        let bytes = rmp_serde::to_vec(vote).expect("Failed to serialize vote");
        store
            .meta_tree
            .insert(KEY_VOTE, bytes)
            .map_err(|e| StorageIOError::write_vote(&e))?;

        // Flush to ensure durability
        store
            .meta_tree
            .flush()
            .map_err(|e| StorageIOError::write_vote(&e))?;

        debug!("Saved vote: {:?}", vote);
        Ok(())
    }

    async fn append<I>(
        &mut self,
        entries: I,
        callback: openraft::storage::LogFlushed<TypeConfig>,
    ) -> Result<(), StorageError<NodeId>>
    where
        I: IntoIterator<Item = Entry<TypeConfig>> + Send,
    {
        let store = self.write().await;

        let mut batch = sled::Batch::default();
        let mut count = 0;

        for entry in entries {
            let key = SledLogStore::index_to_key(entry.log_id.index);
            let value = rmp_serde::to_vec(&entry).expect("Failed to serialize entry");
            batch.insert(&key, value);
            count += 1;
        }

        if count > 0 {
            store
                .log_tree
                .apply_batch(batch)
                .map_err(|e| StorageIOError::write_logs(&e))?;

            // Flush to ensure durability before signaling completion
            store
                .log_tree
                .flush()
                .map_err(|e| StorageIOError::write_logs(&e))?;

            debug!(count, "Appended log entries to persistent storage");
        }

        // Signal that entries are flushed
        callback.log_io_completed(Ok(()));

        Ok(())
    }

    async fn truncate(&mut self, log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        let store = self.write().await;

        // Find and remove entries >= log_id.index
        let start_key = SledLogStore::index_to_key(log_id.index);
        let keys_to_remove: Vec<_> = store
            .log_tree
            .range(start_key..)
            .filter_map(|r| r.ok().map(|(k, _)| k))
            .collect();

        let count = keys_to_remove.len();
        for key in keys_to_remove {
            store
                .log_tree
                .remove(&key)
                .map_err(|e| StorageIOError::write_logs(&e))?;
        }

        if count > 0 {
            store
                .log_tree
                .flush()
                .map_err(|e| StorageIOError::write_logs(&e))?;
            debug!(from_index = log_id.index, count, "Truncated log entries");
        }

        Ok(())
    }

    async fn purge(&mut self, log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        let store = self.write().await;

        // Find and remove entries <= log_id.index
        let end_key = SledLogStore::index_to_key(log_id.index + 1);
        let keys_to_remove: Vec<_> = store
            .log_tree
            .range(..end_key)
            .filter_map(|r| r.ok().map(|(k, _)| k))
            .collect();

        let count = keys_to_remove.len();
        for key in keys_to_remove {
            store
                .log_tree
                .remove(&key)
                .map_err(|e| StorageIOError::write_logs(&e))?;
        }

        // Update last purged
        store
            .set_last_purged(log_id)
            .map_err(|e| StorageIOError::write_logs(&e))?;

        if count > 0 {
            store
                .log_tree
                .flush()
                .map_err(|e| StorageIOError::write_logs(&e))?;
            debug!(through_index = log_id.index, count, "Purged log entries");
        }

        Ok(())
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogId<NodeId>>,
    ) -> Result<(), StorageError<NodeId>> {
        let store = self.write().await;

        match committed {
            Some(log_id) => {
                let bytes = rmp_serde::to_vec(&log_id).expect("Failed to serialize log_id");
                store
                    .meta_tree
                    .insert(KEY_COMMITTED, bytes)
                    .map_err(|e| StorageIOError::write_logs(&e))?;
            }
            None => {
                store
                    .meta_tree
                    .remove(KEY_COMMITTED)
                    .map_err(|e| StorageIOError::write_logs(&e))?;
            }
        }

        store
            .meta_tree
            .flush()
            .map_err(|e| StorageIOError::write_logs(&e))?;

        Ok(())
    }

    async fn read_committed(&mut self) -> Result<Option<LogId<NodeId>>, StorageError<NodeId>> {
        let store = self.read().await;

        let committed = store
            .meta_tree
            .get(KEY_COMMITTED)
            .map_err(|e| StorageIOError::read_logs(&e))?
            .and_then(|v| rmp_serde::from_slice(&v).ok());

        Ok(committed)
    }
}

/// Statistics for the persistent log store
#[derive(Debug, Clone)]
pub struct SledLogStats {
    /// Number of log entries
    pub log_entry_count: usize,
    /// Size of log tree in bytes (approximate)
    pub log_size_bytes: u64,
    /// Last log index
    pub last_log_index: Option<u64>,
    /// Last purged log index
    pub last_purged_index: Option<u64>,
    /// Committed log index
    pub committed_index: Option<u64>,
}

impl SledLogStore {
    /// Get statistics about the persistent log store
    pub fn stats(&self) -> SledLogStats {
        let last_log_index = self.last_log_id().map(|l| l.index);
        let last_purged_index = self.get_last_purged().map(|l| l.index);
        let committed_index = self
            .meta_tree
            .get(KEY_COMMITTED)
            .ok()
            .flatten()
            .and_then(|v| rmp_serde::from_slice::<LogId<NodeId>>(&v).ok())
            .map(|l| l.index);

        // Estimate size by summing entry sizes
        let log_size_bytes: u64 = self
            .log_tree
            .iter()
            .filter_map(|r| r.ok().map(|(k, v)| (k.len() + v.len()) as u64))
            .sum();

        SledLogStats {
            log_entry_count: self.log_tree.len(),
            log_size_bytes,
            last_log_index,
            last_purged_index,
            committed_index,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openraft::EntryPayload;
    use tempfile::TempDir;

    fn create_test_store() -> (TempDir, RwLock<SledLogStore>) {
        let temp_dir = TempDir::new().unwrap();
        let store = SledLogStore::open(temp_dir.path().join("raft")).unwrap();
        (temp_dir, RwLock::new(store))
    }

    fn create_entry(index: u64) -> Entry<TypeConfig> {
        Entry {
            log_id: LogId::new(openraft::CommittedLeaderId::new(1, 1), index),
            payload: EntryPayload::Blank,
        }
    }

    #[tokio::test]
    async fn test_persistent_log_store_basic() {
        let (_temp, mut store) = create_test_store();

        // Initially empty
        let state = store.get_log_state().await.unwrap();
        assert!(state.last_log_id.is_none());
        assert!(state.last_purged_log_id.is_none());

        // Save vote
        let vote = Vote::new(1, 1);
        store.save_vote(&vote).await.unwrap();

        let read_vote = store.read_vote().await.unwrap();
        assert_eq!(read_vote, Some(vote));
    }

    #[tokio::test]
    async fn test_persistent_log_append() {
        let (_temp, store) = create_test_store();

        // Append entries
        {
            let inner = store.write().await;
            for i in 1..=5 {
                let key = SledLogStore::index_to_key(i);
                let entry = create_entry(i);
                let value = rmp_serde::to_vec(&entry).unwrap();
                inner.log_tree.insert(key, value).unwrap();
            }
        }

        // Verify state
        let mut store_mut = store;
        let state = store_mut.get_log_state().await.unwrap();
        assert_eq!(state.last_log_id.unwrap().index, 5);
    }

    #[tokio::test]
    async fn test_persistent_log_truncate() {
        let (_temp, store) = create_test_store();

        // Insert entries
        {
            let inner = store.write().await;
            for i in 1..=5 {
                let key = SledLogStore::index_to_key(i);
                let entry = create_entry(i);
                let value = rmp_serde::to_vec(&entry).unwrap();
                inner.log_tree.insert(key, value).unwrap();
            }
        }

        let mut store_mut = store;

        // Truncate from index 3
        store_mut
            .truncate(LogId::new(openraft::CommittedLeaderId::new(1, 1), 3))
            .await
            .unwrap();

        // Verify only indices 1, 2 remain
        let state = store_mut.get_log_state().await.unwrap();
        assert_eq!(state.last_log_id.unwrap().index, 2);
    }

    #[tokio::test]
    async fn test_persistent_log_purge() {
        let (_temp, store) = create_test_store();

        // Insert entries
        {
            let inner = store.write().await;
            for i in 1..=5 {
                let key = SledLogStore::index_to_key(i);
                let entry = create_entry(i);
                let value = rmp_serde::to_vec(&entry).unwrap();
                inner.log_tree.insert(key, value).unwrap();
            }
        }

        let mut store_mut = store;

        // Purge through index 3
        store_mut
            .purge(LogId::new(openraft::CommittedLeaderId::new(1, 1), 3))
            .await
            .unwrap();

        // Verify state
        let state = store_mut.get_log_state().await.unwrap();
        assert_eq!(state.last_purged_log_id.unwrap().index, 3);
        assert_eq!(state.last_log_id.unwrap().index, 5);
    }

    #[tokio::test]
    async fn test_persistent_log_committed() {
        let (_temp, mut store) = create_test_store();

        // Initially no committed
        let committed = store.read_committed().await.unwrap();
        assert!(committed.is_none());

        // Save committed
        let log_id = LogId::new(openraft::CommittedLeaderId::new(1, 1), 5);
        store.save_committed(Some(log_id)).await.unwrap();

        // Read back
        let committed = store.read_committed().await.unwrap();
        assert_eq!(committed.unwrap().index, 5);
    }

    #[tokio::test]
    async fn test_persistent_log_try_get_entries() {
        let (_temp, store) = create_test_store();

        // Insert entries
        {
            let inner = store.write().await;
            for i in 1..=5 {
                let key = SledLogStore::index_to_key(i);
                let entry = create_entry(i);
                let value = rmp_serde::to_vec(&entry).unwrap();
                inner.log_tree.insert(key, value).unwrap();
            }
        }

        let mut store_mut = store;

        // Get range
        let entries = store_mut.try_get_log_entries(2..=4).await.unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].log_id.index, 2);
        assert_eq!(entries[1].log_id.index, 3);
        assert_eq!(entries[2].log_id.index, 4);
    }

    #[tokio::test]
    async fn test_persistence_across_reopen() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("raft");

        // First session: write data
        {
            let store = SledLogStore::open(&db_path).unwrap();
            let mut store = RwLock::new(store);

            // Save vote
            let vote = Vote::new(2, 1);
            store.save_vote(&vote).await.unwrap();

            // Save committed
            let log_id = LogId::new(openraft::CommittedLeaderId::new(2, 1), 10);
            store.save_committed(Some(log_id)).await.unwrap();

            // Add log entry
            {
                let inner = store.write().await;
                let key = SledLogStore::index_to_key(10);
                let entry: Entry<TypeConfig> = Entry {
                    log_id: LogId::new(openraft::CommittedLeaderId::new(2, 1), 10),
                    payload: EntryPayload::Blank,
                };
                let value = rmp_serde::to_vec(&entry).unwrap();
                inner.log_tree.insert(key, value).unwrap();
                inner.flush().unwrap();
            }
        }

        // Second session: verify data persisted
        {
            let store = SledLogStore::open(&db_path).unwrap();
            let mut store = RwLock::new(store);

            // Verify vote
            let vote = store.read_vote().await.unwrap().unwrap();
            assert_eq!(vote.leader_id().voted_for(), Some(1));

            // Verify committed
            let committed = store.read_committed().await.unwrap().unwrap();
            assert_eq!(committed.index, 10);

            // Verify log entry
            let state = store.get_log_state().await.unwrap();
            assert_eq!(state.last_log_id.unwrap().index, 10);
        }
    }

    #[tokio::test]
    async fn test_stats() {
        let (_temp, store) = create_test_store();

        // Insert some entries
        {
            let inner = store.write().await;
            for i in 1..=10 {
                let key = SledLogStore::index_to_key(i);
                let entry = create_entry(i);
                let value = rmp_serde::to_vec(&entry).unwrap();
                inner.log_tree.insert(key, value).unwrap();
            }
        }

        let inner = store.read().await;
        let stats = inner.stats();
        assert_eq!(stats.log_entry_count, 10);
        assert_eq!(stats.last_log_index, Some(10));
    }
}
