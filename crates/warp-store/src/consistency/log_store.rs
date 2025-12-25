//! In-memory log storage for Raft
//!
//! Provides persistent storage for Raft log entries. This implementation
//! uses in-memory storage suitable for development and testing. For production,
//! this can be replaced with disk-backed storage.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::ops::RangeBounds;

use openraft::{
    Entry, LogId, LogState, StorageError, Vote,
    storage::{RaftLogReader, RaftLogStorage},
};
use tokio::sync::RwLock;

use super::types::*;

/// In-memory log store
///
/// Stores Raft log entries in memory. For production use, consider
/// implementing disk-based persistence.
pub struct MemLogStore {
    /// Last purged log ID
    last_purged_log_id: Option<LogId<NodeId>>,

    /// The vote (current term and voted_for)
    vote: Option<Vote<NodeId>>,

    /// Log entries indexed by log index
    log: BTreeMap<u64, Entry<TypeConfig>>,

    /// Committed index
    committed: Option<LogId<NodeId>>,
}

impl MemLogStore {
    /// Create a new empty log store
    pub fn new() -> Self {
        Self {
            last_purged_log_id: None,
            vote: None,
            log: BTreeMap::new(),
            committed: None,
        }
    }

    /// Get the last log ID in the store
    fn last_log_id(&self) -> Option<LogId<NodeId>> {
        self.log.iter().next_back().map(|(_, entry)| entry.log_id)
    }
}

impl Default for MemLogStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RaftLogReader<TypeConfig> for RwLock<MemLogStore> {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: RB,
    ) -> Result<Vec<Entry<TypeConfig>>, StorageError<NodeId>> {
        let store = self.read().await;

        let entries: Vec<_> = store
            .log
            .range(range)
            .map(|(_, entry)| entry.clone())
            .collect();

        Ok(entries)
    }
}

impl RaftLogStorage<TypeConfig> for RwLock<MemLogStore> {
    type LogReader = Self;

    async fn read_vote(&mut self) -> Result<Option<Vote<NodeId>>, StorageError<NodeId>> {
        let store = self.read().await;
        Ok(store.vote)
    }

    async fn get_log_state(&mut self) -> Result<LogState<TypeConfig>, StorageError<NodeId>> {
        let store = self.read().await;

        let last_purged = store.last_purged_log_id;
        let last = store.last_log_id();

        Ok(LogState {
            last_purged_log_id: last_purged,
            last_log_id: last,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        // This is a simplified implementation - we return a reference to self
        // In a real implementation, you'd create a separate reader handle
        panic!("get_log_reader called - use try_get_log_entries directly")
    }

    async fn save_vote(&mut self, vote: &Vote<NodeId>) -> Result<(), StorageError<NodeId>> {
        let mut store = self.write().await;
        store.vote = Some(*vote);
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
        let mut store = self.write().await;

        for entry in entries {
            store.log.insert(entry.log_id.index, entry);
        }

        // Signal that entries are flushed (in-memory, so immediate)
        callback.log_io_completed(Ok(()));

        Ok(())
    }

    async fn truncate(&mut self, log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        let mut store = self.write().await;

        let keys_to_remove: Vec<_> = store
            .log
            .range(log_id.index..)
            .map(|(k, _)| *k)
            .collect();

        for key in keys_to_remove {
            store.log.remove(&key);
        }

        Ok(())
    }

    async fn purge(&mut self, log_id: LogId<NodeId>) -> Result<(), StorageError<NodeId>> {
        let mut store = self.write().await;

        let keys_to_remove: Vec<_> = store
            .log
            .range(..=log_id.index)
            .map(|(k, _)| *k)
            .collect();

        for key in keys_to_remove {
            store.log.remove(&key);
        }

        store.last_purged_log_id = Some(log_id);

        Ok(())
    }

    async fn save_committed(
        &mut self,
        committed: Option<LogId<NodeId>>,
    ) -> Result<(), StorageError<NodeId>> {
        let mut store = self.write().await;
        store.committed = committed;
        Ok(())
    }

    async fn read_committed(&mut self) -> Result<Option<LogId<NodeId>>, StorageError<NodeId>> {
        let store = self.read().await;
        Ok(store.committed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openraft::EntryPayload;

    #[tokio::test]
    async fn test_log_store_basic() {
        let mut store = RwLock::new(MemLogStore::new());

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
    async fn test_log_store_direct_operations() {
        let store = RwLock::new(MemLogStore::new());

        // Directly insert entries for testing (bypassing append which needs callback)
        {
            let mut inner = store.write().await;
            inner.log.insert(
                1,
                Entry {
                    log_id: LogId::new(openraft::CommittedLeaderId::new(1, 1), 1),
                    payload: EntryPayload::Blank,
                },
            );
            inner.log.insert(
                2,
                Entry {
                    log_id: LogId::new(openraft::CommittedLeaderId::new(1, 1), 2),
                    payload: EntryPayload::Blank,
                },
            );
        }

        // Verify state via public interface
        let mut store_mut = store;
        let state = store_mut.get_log_state().await.unwrap();
        assert_eq!(state.last_log_id.unwrap().index, 2);
    }

    #[tokio::test]
    async fn test_log_store_truncate() {
        let store = RwLock::new(MemLogStore::new());

        // Directly insert entries
        {
            let mut inner = store.write().await;
            for i in 1..=3 {
                inner.log.insert(
                    i,
                    Entry {
                        log_id: LogId::new(openraft::CommittedLeaderId::new(1, 1), i),
                        payload: EntryPayload::Blank,
                    },
                );
            }
        }

        let mut store_mut = store;

        // Truncate from index 2
        store_mut
            .truncate(LogId::new(openraft::CommittedLeaderId::new(1, 1), 2))
            .await
            .unwrap();

        // Verify only index 1 remains
        let state = store_mut.get_log_state().await.unwrap();
        assert_eq!(state.last_log_id.unwrap().index, 1);
    }

    #[tokio::test]
    async fn test_log_store_purge() {
        let store = RwLock::new(MemLogStore::new());

        // Directly insert entries
        {
            let mut inner = store.write().await;
            for i in 1..=3 {
                inner.log.insert(
                    i,
                    Entry {
                        log_id: LogId::new(openraft::CommittedLeaderId::new(1, 1), i),
                        payload: EntryPayload::Blank,
                    },
                );
            }
        }

        let mut store_mut = store;

        // Purge up to index 2
        store_mut
            .purge(LogId::new(openraft::CommittedLeaderId::new(1, 1), 2))
            .await
            .unwrap();

        // Verify state
        let state = store_mut.get_log_state().await.unwrap();
        assert_eq!(state.last_purged_log_id.unwrap().index, 2);
        assert_eq!(state.last_log_id.unwrap().index, 3);
    }

    #[tokio::test]
    async fn test_log_store_committed() {
        let mut store = RwLock::new(MemLogStore::new());

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
    async fn test_log_store_try_get_log_entries() {
        let store = RwLock::new(MemLogStore::new());

        // Directly insert entries
        {
            let mut inner = store.write().await;
            for i in 1..=5 {
                inner.log.insert(
                    i,
                    Entry {
                        log_id: LogId::new(openraft::CommittedLeaderId::new(1, 1), i),
                        payload: EntryPayload::Blank,
                    },
                );
            }
        }

        let mut store_mut = store;

        // Get range of entries
        let entries = store_mut.try_get_log_entries(2..=4).await.unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].log_id.index, 2);
        assert_eq!(entries[1].log_id.index, 3);
        assert_eq!(entries[2].log_id.index, 4);
    }
}
