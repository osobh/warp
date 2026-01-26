//! Shuttle-based concurrency tests for NetworkManager
//!
//! These tests use shuttle to randomly explore interleavings of
//! concurrent operations on the network manager.

#![allow(dead_code)]
#![allow(clippy::mutex_atomic)]

use shuttle::sync::atomic::{AtomicU32, Ordering};
use shuttle::sync::{Mutex, RwLock};
use shuttle::thread;
use std::collections::HashMap;
use std::sync::Arc;

/// Simplified network state for shuttle testing
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

/// Simplified peer entry
#[derive(Debug, Clone)]
struct PeerEntry {
    public_key: [u8; 32],
    status: PeerStatus,
}

/// Simplified manager state
struct ShuttleManagerState {
    state: NetworkState,
    peers: HashMap<[u8; 32], PeerEntry>,
    peer_count: u32,
    connected_count: u32,
}

impl ShuttleManagerState {
    fn new() -> Self {
        Self {
            state: NetworkState::Initializing,
            peers: HashMap::new(),
            peer_count: 0,
            connected_count: 0,
        }
    }
}

/// Simplified NetworkManager for shuttle testing
struct ShuttleNetworkManager {
    state: RwLock<ShuttleManagerState>,
    event_count: AtomicU32,
    /// Track operation history for invariant checking
    operation_log: Mutex<Vec<String>>,
}

impl ShuttleNetworkManager {
    fn new() -> Self {
        Self {
            state: RwLock::new(ShuttleManagerState::new()),
            event_count: AtomicU32::new(0),
            operation_log: Mutex::new(Vec::new()),
        }
    }

    fn log_operation(&self, op: &str) {
        let mut log = self.operation_log.lock().unwrap();
        log.push(op.to_string());
    }

    fn add_peer(&self, public_key: [u8; 32]) -> bool {
        self.log_operation(&format!("add_peer_start:{:?}", &public_key[0..4]));

        // Check if exists (read lock)
        {
            let state = self.state.read().unwrap();
            if state.peers.contains_key(&public_key) {
                self.log_operation("add_peer_exists");
                return false;
            }
        }

        // Yield to allow other threads to interleave
        shuttle::thread::yield_now();

        // Add peer (write lock)
        let added = {
            let mut state = self.state.write().unwrap();
            if state.peers.contains_key(&public_key) {
                false
            } else {
                state.peers.insert(
                    public_key,
                    PeerEntry {
                        public_key,
                        status: PeerStatus::Discovered,
                    },
                );
                state.peer_count += 1;
                true
            }
        };

        if added {
            self.emit_event();
            self.update_network_state();
            self.log_operation("add_peer_success");
        } else {
            self.log_operation("add_peer_race_lost");
        }
        added
    }

    fn remove_peer(&self, public_key: &[u8; 32]) -> bool {
        self.log_operation(&format!("remove_peer_start:{:?}", &public_key[0..4]));

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
            self.log_operation("remove_peer_success");
        } else {
            self.log_operation("remove_peer_not_found");
        }
        removed
    }

    fn connect_peer(&self, public_key: &[u8; 32]) -> bool {
        self.log_operation(&format!("connect_peer:{:?}", &public_key[0..4]));

        let connected = {
            let mut state = self.state.write().unwrap();
            if let Some(peer) = state.peers.get_mut(public_key) {
                if peer.status != PeerStatus::Connected {
                    peer.status = PeerStatus::Connected;
                    state.connected_count += 1;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if connected {
            self.emit_event();
            self.update_network_state();
        }
        connected
    }

    fn disconnect_peer(&self, public_key: &[u8; 32]) -> bool {
        self.log_operation(&format!("disconnect_peer:{:?}", &public_key[0..4]));

        let disconnected = {
            let mut state = self.state.write().unwrap();
            if let Some(peer) = state.peers.get_mut(public_key) {
                if peer.status == PeerStatus::Connected {
                    peer.status = PeerStatus::Disconnected;
                    state.connected_count -= 1;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if disconnected {
            self.emit_event();
            self.update_network_state();
        }
        disconnected
    }

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

    fn check_invariants(&self) {
        let state = self.state.read().unwrap();

        // Invariant 1: connected_count <= peer_count
        assert!(
            state.connected_count <= state.peer_count,
            "connected_count ({}) > peer_count ({})",
            state.connected_count,
            state.peer_count
        );

        // Invariant 2: peer_count matches HashMap size
        assert_eq!(
            state.peer_count as usize,
            state.peers.len(),
            "peer_count mismatch"
        );

        // Invariant 3: connected_count matches actual connected peers
        let actual_connected = state
            .peers
            .values()
            .filter(|p| p.status == PeerStatus::Connected)
            .count();
        assert_eq!(
            state.connected_count as usize, actual_connected,
            "connected_count mismatch"
        );

        // Invariant 4: state is consistent with counts
        match state.state {
            NetworkState::FullMesh => {
                assert!(state.peer_count > 0 && state.connected_count == state.peer_count);
            }
            NetworkState::Degraded => {
                assert!(state.connected_count > 0 && state.connected_count < state.peer_count);
            }
            NetworkState::DiscoveryOnly => {
                assert!(state.connected_count == 0 || state.peer_count == 0);
            }
            _ => {}
        }
    }
}

#[test]
fn test_many_concurrent_adds() {
    shuttle::check_random(
        || {
            let manager = Arc::new(ShuttleNetworkManager::new());
            let handles: Vec<_> = (0..4)
                .map(|i| {
                    let m = manager.clone();
                    thread::spawn(move || {
                        let key = [i as u8; 32];
                        m.add_peer(key)
                    })
                })
                .collect();

            for h in handles {
                h.join().unwrap();
            }

            manager.check_invariants();
            assert_eq!(manager.get_peer_count(), 4);
        },
        1000,
    );
}

#[test]
fn test_add_connect_disconnect_cycle() {
    shuttle::check_random(
        || {
            let manager = Arc::new(ShuttleNetworkManager::new());
            let key: [u8; 32] = [1; 32];

            // Add peer first
            manager.add_peer(key);

            let m1 = manager.clone();
            let t1 = thread::spawn(move || {
                m1.connect_peer(&[1; 32]);
            });

            let m2 = manager.clone();
            let t2 = thread::spawn(move || {
                m2.disconnect_peer(&[1; 32]);
            });

            let m3 = manager.clone();
            let t3 = thread::spawn(move || {
                m3.connect_peer(&[1; 32]);
            });

            t1.join().unwrap();
            t2.join().unwrap();
            t3.join().unwrap();

            manager.check_invariants();
        },
        1000,
    );
}

#[test]
fn test_concurrent_add_remove_same_peer() {
    shuttle::check_random(
        || {
            let manager = Arc::new(ShuttleNetworkManager::new());

            let m1 = manager.clone();
            let t1 = thread::spawn(move || m1.add_peer([1; 32]));

            let m2 = manager.clone();
            let t2 = thread::spawn(move || m2.remove_peer(&[1; 32]));

            let m3 = manager.clone();
            let t3 = thread::spawn(move || m3.add_peer([1; 32]));

            t1.join().unwrap();
            t2.join().unwrap();
            t3.join().unwrap();

            manager.check_invariants();
            // Peer count should be 0 or 1
            assert!(manager.get_peer_count() <= 1);
        },
        1000,
    );
}

#[test]
fn test_state_transitions_under_load() {
    shuttle::check_random(
        || {
            let manager = Arc::new(ShuttleNetworkManager::new());

            // Add several peers
            for i in 0..3 {
                manager.add_peer([i; 32]);
            }

            // Concurrent connects and disconnects
            let handles: Vec<_> = (0..3)
                .flat_map(|i| {
                    let m1 = manager.clone();
                    let m2 = manager.clone();
                    vec![
                        thread::spawn(move || m1.connect_peer(&[i; 32])),
                        thread::spawn(move || m2.disconnect_peer(&[i; 32])),
                    ]
                })
                .collect();

            for h in handles {
                h.join().unwrap();
            }

            manager.check_invariants();
        },
        1000,
    );
}

#[test]
fn test_full_lifecycle() {
    shuttle::check_random(
        || {
            let manager = Arc::new(ShuttleNetworkManager::new());

            // Phase 1: Add peers
            let m1 = manager.clone();
            let t1 = thread::spawn(move || {
                m1.add_peer([1; 32]);
                m1.add_peer([2; 32]);
            });

            // Phase 2: Connect peers
            let m2 = manager.clone();
            let t2 = thread::spawn(move || {
                shuttle::thread::yield_now();
                m2.connect_peer(&[1; 32]);
                m2.connect_peer(&[2; 32]);
            });

            // Phase 3: Remove one peer
            let m3 = manager.clone();
            let t3 = thread::spawn(move || {
                shuttle::thread::yield_now();
                shuttle::thread::yield_now();
                m3.remove_peer(&[1; 32]);
            });

            t1.join().unwrap();
            t2.join().unwrap();
            t3.join().unwrap();

            manager.check_invariants();
        },
        1000,
    );
}

#[test]
fn test_double_add_protection() {
    shuttle::check_random(
        || {
            let manager = Arc::new(ShuttleNetworkManager::new());
            let key: [u8; 32] = [42; 32];

            // Multiple threads try to add the same peer
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    let m = manager.clone();
                    let k = key;
                    thread::spawn(move || m.add_peer(k))
                })
                .collect();

            let results: Vec<bool> = handles.into_iter().map(|h| h.join().unwrap()).collect();

            // Exactly one should succeed
            let successes = results.iter().filter(|&&r| r).count();
            assert_eq!(successes, 1, "Exactly one add should succeed");
            assert_eq!(manager.get_peer_count(), 1);
            manager.check_invariants();
        },
        1000,
    );
}
