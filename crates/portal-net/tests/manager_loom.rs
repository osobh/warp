//! Loom-based concurrency tests for NetworkManager state transitions
//!
//! These tests use loom to exhaustively check all possible interleavings
//! of concurrent operations on the network manager state.

#![allow(dead_code)]

use loom::sync::atomic::{AtomicU32, Ordering};
use loom::sync::{Arc, RwLock};
use loom::thread;
use std::collections::HashMap;

/// Simplified network state for loom testing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NetworkState {
    Initializing,
    DiscoveryOnly,
    HubConnected,
    FullMesh,
    Degraded,
    Offline,
}

/// Simplified peer status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PeerStatus {
    Unknown,
    Discovered,
    Connected,
    Disconnected,
}

/// Simplified peer entry for testing
#[derive(Debug, Clone)]
struct PeerEntry {
    public_key: [u8; 32],
    status: PeerStatus,
}

/// Simplified manager state for loom testing
struct LoomManagerState {
    state: NetworkState,
    peers: HashMap<[u8; 32], PeerEntry>,
    peer_count: u32,
    connected_count: u32,
}

impl LoomManagerState {
    fn new() -> Self {
        Self {
            state: NetworkState::Initializing,
            peers: HashMap::new(),
            peer_count: 0,
            connected_count: 0,
        }
    }
}

/// Simplified NetworkManager for loom testing
struct LoomNetworkManager {
    state: RwLock<LoomManagerState>,
    event_count: AtomicU32,
}

impl LoomNetworkManager {
    fn new() -> Self {
        Self {
            state: RwLock::new(LoomManagerState::new()),
            event_count: AtomicU32::new(0),
        }
    }

    /// Add a peer - simulates the TOCTOU pattern in real manager
    fn add_peer(&self, public_key: [u8; 32]) -> bool {
        // First check if peer exists (read lock)
        {
            let state = self.state.read().unwrap();
            if state.peers.contains_key(&public_key) {
                return false;
            }
        }
        // Release read lock, acquire write lock
        // This is where TOCTOU can occur
        {
            let mut state = self.state.write().unwrap();
            // Double-check pattern
            if state.peers.contains_key(&public_key) {
                return false;
            }
            state.peers.insert(
                public_key,
                PeerEntry {
                    public_key,
                    status: PeerStatus::Discovered,
                },
            );
            state.peer_count += 1;
        }
        self.emit_event();
        self.update_network_state();
        true
    }

    /// Remove a peer
    fn remove_peer(&self, public_key: &[u8; 32]) -> bool {
        let removed = {
            let mut state = self.state.write().unwrap();
            if let Some(peer) = state.peers.remove(public_key) {
                state.peer_count -= 1;
                if peer.status == PeerStatus::Connected {
                    state.connected_count -= 1;
                }
                true
            } else {
                false
            }
        };
        if removed {
            self.emit_event();
            self.update_network_state();
        }
        removed
    }

    /// Update peer status
    fn update_peer_status(&self, public_key: &[u8; 32], status: PeerStatus) -> bool {
        let updated = {
            let mut state = self.state.write().unwrap();
            if let Some(peer) = state.peers.get_mut(public_key) {
                let was_connected = peer.status == PeerStatus::Connected;
                let is_connected = status == PeerStatus::Connected;

                peer.status = status;

                // Update connected count
                if was_connected && !is_connected {
                    state.connected_count -= 1;
                } else if !was_connected && is_connected {
                    state.connected_count += 1;
                }
                true
            } else {
                false
            }
        };
        if updated {
            self.emit_event();
            self.update_network_state();
        }
        updated
    }

    /// Update network state based on current peer status
    fn update_network_state(&self) {
        let mut state = self.state.write().unwrap();
        let new_state = if state.peer_count == 0 {
            NetworkState::DiscoveryOnly
        } else if state.connected_count == state.peer_count {
            NetworkState::FullMesh
        } else if state.connected_count > 0 {
            NetworkState::Degraded
        } else {
            NetworkState::DiscoveryOnly
        };
        state.state = new_state;
    }

    fn emit_event(&self) {
        self.event_count.fetch_add(1, Ordering::SeqCst);
    }

    fn get_state(&self) -> NetworkState {
        self.state.read().unwrap().state
    }

    fn get_peer_count(&self) -> u32 {
        self.state.read().unwrap().peer_count
    }

    fn get_connected_count(&self) -> u32 {
        self.state.read().unwrap().connected_count
    }

    fn get_event_count(&self) -> u32 {
        self.event_count.load(Ordering::SeqCst)
    }
}

#[test]
fn test_concurrent_add_same_peer() {
    loom::model(|| {
        let manager = Arc::new(LoomNetworkManager::new());
        let key: [u8; 32] = [1; 32];

        let m1 = manager.clone();
        let k1 = key;
        let t1 = thread::spawn(move || m1.add_peer(k1));

        let m2 = manager.clone();
        let k2 = key;
        let t2 = thread::spawn(move || m2.add_peer(k2));

        let r1 = t1.join().unwrap();
        let r2 = t2.join().unwrap();

        // Only one should succeed (no duplicate peers)
        assert!(r1 ^ r2, "Exactly one add should succeed");
        assert_eq!(manager.get_peer_count(), 1);
    });
}

#[test]
fn test_concurrent_add_different_peers() {
    loom::model(|| {
        let manager = Arc::new(LoomNetworkManager::new());

        let m1 = manager.clone();
        let t1 = thread::spawn(move || m1.add_peer([1; 32]));

        let m2 = manager.clone();
        let t2 = thread::spawn(move || m2.add_peer([2; 32]));

        t1.join().unwrap();
        t2.join().unwrap();

        // Both should be added
        assert_eq!(manager.get_peer_count(), 2);
    });
}

#[test]
fn test_add_remove_race() {
    loom::model(|| {
        let manager = Arc::new(LoomNetworkManager::new());
        let key: [u8; 32] = [1; 32];

        // Pre-add the peer
        manager.add_peer(key);

        let m1 = manager.clone();
        let k1 = key;
        let t1 = thread::spawn(move || m1.remove_peer(&k1));

        let m2 = manager.clone();
        let k2 = key;
        let t2 = thread::spawn(move || m2.add_peer(k2));

        let removed = t1.join().unwrap();
        let added = t2.join().unwrap();

        // State should be consistent
        let count = manager.get_peer_count();
        if removed && added {
            // Removed first, then added - count should be 1
            assert_eq!(count, 1);
        } else if removed && !added {
            // Only removed - count should be 0
            assert_eq!(count, 0);
        } else if !removed && added {
            // Shouldn't happen since we pre-added
            unreachable!();
        }
    });
}

#[test]
fn test_concurrent_status_updates() {
    loom::model(|| {
        let manager = Arc::new(LoomNetworkManager::new());
        let key: [u8; 32] = [1; 32];

        // Add peer first
        manager.add_peer(key);

        let m1 = manager.clone();
        let k1 = key;
        let t1 = thread::spawn(move || {
            m1.update_peer_status(&k1, PeerStatus::Connected);
        });

        let m2 = manager.clone();
        let k2 = key;
        let t2 = thread::spawn(move || {
            m2.update_peer_status(&k2, PeerStatus::Disconnected);
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // Connected count should be consistent (0 or 1)
        let connected = manager.get_connected_count();
        assert!(connected <= 1);
    });
}

#[test]
fn test_network_state_consistency() {
    loom::model(|| {
        let manager = Arc::new(LoomNetworkManager::new());

        let m1 = manager.clone();
        let t1 = thread::spawn(move || {
            m1.add_peer([1; 32]);
            m1.update_peer_status(&[1; 32], PeerStatus::Connected);
        });

        let m2 = manager.clone();
        let t2 = thread::spawn(move || {
            m2.add_peer([2; 32]);
            m2.update_peer_status(&[2; 32], PeerStatus::Connected);
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // Final state should be consistent with peer/connected counts
        let state = manager.get_state();
        let peers = manager.get_peer_count();
        let connected = manager.get_connected_count();

        match state {
            NetworkState::FullMesh => {
                assert_eq!(peers, connected);
                assert!(peers > 0);
            }
            NetworkState::Degraded => {
                assert!(connected > 0);
                assert!(connected < peers);
            }
            NetworkState::DiscoveryOnly => {
                assert!(connected == 0 || peers == 0);
            }
            _ => {}
        }
    });
}

#[test]
fn test_event_ordering() {
    loom::model(|| {
        let manager = Arc::new(LoomNetworkManager::new());

        let m1 = manager.clone();
        let t1 = thread::spawn(move || {
            m1.add_peer([1; 32]);
        });

        let m2 = manager.clone();
        let t2 = thread::spawn(move || {
            m2.add_peer([2; 32]);
        });

        t1.join().unwrap();
        t2.join().unwrap();

        // Each add emits an event
        let events = manager.get_event_count();
        assert!(events >= 2, "Should have at least 2 events");
    });
}

/// Test that demonstrates the lock downgrade pattern safety
#[test]
fn test_read_write_lock_upgrade() {
    loom::model(|| {
        let manager = Arc::new(LoomNetworkManager::new());
        let key: [u8; 32] = [1; 32];

        // One thread adds a peer (read-check then write)
        let m1 = manager.clone();
        let k1 = key;
        let t1 = thread::spawn(move || m1.add_peer(k1));

        // Another thread reads state while first is between read and write
        let m2 = manager.clone();
        let t2 = thread::spawn(move || m2.get_peer_count());

        t1.join().unwrap();
        let count_during = t2.join().unwrap();

        // Count during could be 0 or 1 depending on interleaving
        assert!(count_during <= 1);

        // Final count should be 1
        assert_eq!(manager.get_peer_count(), 1);
    });
}
