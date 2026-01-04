//! Raft network layer
//!
//! Handles communication between Raft nodes. This implementation uses
//! tokio channels for in-process communication, suitable for testing
//! and single-machine deployments. For distributed deployments, this
//! can be extended to use TCP/QUIC/gRPC.

use std::sync::Arc;

use dashmap::DashMap;
use openraft::{
    BasicNode,
    error::{InstallSnapshotError, RPCError, RaftError, Unreachable},
    network::{RPCOption, RaftNetwork, RaftNetworkFactory as RaftNetworkFactoryTrait},
    raft::{
        AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest,
        InstallSnapshotResponse, VoteRequest, VoteResponse,
    },
};
use tokio::sync::mpsc;

use super::types::*;

/// Simple error for network issues
#[derive(Debug)]
struct NetworkError(String);

impl std::fmt::Display for NetworkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for NetworkError {}

/// Message types for Raft RPC
#[derive(Debug)]
pub enum RaftMessage {
    /// AppendEntries RPC
    AppendEntries {
        /// The request
        request: AppendEntriesRequest<TypeConfig>,
        /// Channel to send response
        response_tx: tokio::sync::oneshot::Sender<AppendEntriesResponse<NodeId>>,
    },

    /// Vote RPC
    Vote {
        /// The request
        request: VoteRequest<NodeId>,
        /// Channel to send response
        response_tx: tokio::sync::oneshot::Sender<VoteResponse<NodeId>>,
    },

    /// InstallSnapshot RPC
    InstallSnapshot {
        /// The request
        request: InstallSnapshotRequest<TypeConfig>,
        /// Channel to send response
        response_tx: tokio::sync::oneshot::Sender<InstallSnapshotResponse<NodeId>>,
    },
}

/// Router for Raft messages between nodes
///
/// In a distributed setup, this would be replaced with actual network connections.
/// For now, it uses in-memory channels.
pub struct RaftRouter {
    /// Message channels for each node
    channels: DashMap<NodeId, mpsc::Sender<RaftMessage>>,
}

impl RaftRouter {
    /// Create a new router
    pub fn new() -> Self {
        Self {
            channels: DashMap::new(),
        }
    }

    /// Register a node's message channel
    pub fn register(&self, node_id: NodeId, sender: mpsc::Sender<RaftMessage>) {
        self.channels.insert(node_id, sender);
    }

    /// Unregister a node
    pub fn unregister(&self, node_id: NodeId) {
        self.channels.remove(&node_id);
    }

    /// Get a sender for a node
    pub fn get_sender(&self, node_id: NodeId) -> Option<mpsc::Sender<RaftMessage>> {
        self.channels.get(&node_id).map(|r| r.value().clone())
    }

    /// Check if a node is registered
    pub fn contains(&self, node_id: NodeId) -> bool {
        self.channels.contains_key(&node_id)
    }

    /// Get all registered node IDs
    pub fn nodes(&self) -> Vec<NodeId> {
        self.channels.iter().map(|r| *r.key()).collect()
    }
}

impl Default for RaftRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Network factory that creates network connections to other nodes
pub struct RaftNetworkFactory {
    /// The router for message passing
    router: Arc<RaftRouter>,
}

impl RaftNetworkFactory {
    /// Create a new network factory
    pub fn new(router: Arc<RaftRouter>) -> Self {
        Self { router }
    }
}

impl RaftNetworkFactoryTrait<TypeConfig> for RaftNetworkFactory {
    type Network = RaftNetworkConnection;

    async fn new_client(&mut self, target: NodeId, _node: &BasicNode) -> Self::Network {
        RaftNetworkConnection {
            target,
            router: Arc::clone(&self.router),
        }
    }
}

/// A network connection to a single Raft node
pub struct RaftNetworkConnection {
    /// Target node ID
    target: NodeId,

    /// Router for message passing
    router: Arc<RaftRouter>,
}

impl RaftNetwork<TypeConfig> for RaftNetworkConnection {
    async fn append_entries(
        &mut self,
        request: AppendEntriesRequest<TypeConfig>,
        _option: RPCOption,
    ) -> Result<AppendEntriesResponse<NodeId>, RPCError<NodeId, BasicNode, RaftError<NodeId>>> {
        let sender = self.router.get_sender(self.target).ok_or_else(|| {
            RPCError::Unreachable(Unreachable::new(&NetworkError(format!(
                "Node {} not found",
                self.target
            ))))
        })?;

        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        sender
            .send(RaftMessage::AppendEntries {
                request,
                response_tx,
            })
            .await
            .map_err(|_| {
                RPCError::Unreachable(Unreachable::new(&NetworkError(format!(
                    "Failed to send to node {}",
                    self.target
                ))))
            })?;

        response_rx.await.map_err(|_| {
            RPCError::Unreachable(Unreachable::new(&NetworkError(format!(
                "No response from node {}",
                self.target
            ))))
        })
    }

    async fn install_snapshot(
        &mut self,
        request: InstallSnapshotRequest<TypeConfig>,
        _option: RPCOption,
    ) -> Result<
        InstallSnapshotResponse<NodeId>,
        RPCError<NodeId, BasicNode, RaftError<NodeId, InstallSnapshotError>>,
    > {
        let sender = self.router.get_sender(self.target).ok_or_else(|| {
            RPCError::Unreachable(Unreachable::new(&NetworkError(format!(
                "Node {} not found",
                self.target
            ))))
        })?;

        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        sender
            .send(RaftMessage::InstallSnapshot {
                request,
                response_tx,
            })
            .await
            .map_err(|_| {
                RPCError::Unreachable(Unreachable::new(&NetworkError(format!(
                    "Failed to send to node {}",
                    self.target
                ))))
            })?;

        response_rx.await.map_err(|_| {
            RPCError::Unreachable(Unreachable::new(&NetworkError(format!(
                "No response from node {}",
                self.target
            ))))
        })
    }

    async fn vote(
        &mut self,
        request: VoteRequest<NodeId>,
        _option: RPCOption,
    ) -> Result<VoteResponse<NodeId>, RPCError<NodeId, BasicNode, RaftError<NodeId>>> {
        let sender = self.router.get_sender(self.target).ok_or_else(|| {
            RPCError::Unreachable(Unreachable::new(&NetworkError(format!(
                "Node {} not found",
                self.target
            ))))
        })?;

        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        sender
            .send(RaftMessage::Vote {
                request,
                response_tx,
            })
            .await
            .map_err(|_| {
                RPCError::Unreachable(Unreachable::new(&NetworkError(format!(
                    "Failed to send to node {}",
                    self.target
                ))))
            })?;

        response_rx.await.map_err(|_| {
            RPCError::Unreachable(Unreachable::new(&NetworkError(format!(
                "No response from node {}",
                self.target
            ))))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_basic() {
        let router = RaftRouter::new();

        // Initially empty
        assert!(!router.contains(1));
        assert!(router.nodes().is_empty());

        // Register a node
        let (tx, _rx) = mpsc::channel(10);
        router.register(1, tx);

        assert!(router.contains(1));
        assert_eq!(router.nodes(), vec![1]);

        // Unregister
        router.unregister(1);
        assert!(!router.contains(1));
    }

    #[tokio::test]
    async fn test_network_factory() {
        let router = Arc::new(RaftRouter::new());
        let mut factory = RaftNetworkFactory::new(Arc::clone(&router));

        let node_info = BasicNode {
            addr: "test-node".to_string(),
        };

        let conn = factory.new_client(1, &node_info).await;
        assert_eq!(conn.target, 1);
    }
}
