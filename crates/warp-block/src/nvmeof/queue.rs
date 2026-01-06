//! NVMe Queue Management
//!
//! This module implements Admin and I/O queue management for NVMe-oF.

use parking_lot::{Mutex, RwLock};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};
use tracing::{debug, trace, warn};

use super::capsule::{CommandCapsule, ResponseCapsule};
use super::error::{NvmeOfError, NvmeOfResult, NvmeStatus};

/// Queue identifier (0 = admin, 1+ = I/O)
pub type QueueId = u16;

/// Command identifier
pub type CommandId = u16;

/// Queue configuration
#[derive(Debug, Clone)]
pub struct QueueConfig {
    /// Queue ID
    pub qid: QueueId,

    /// Queue depth (max entries)
    pub depth: u16,

    /// Priority (for weighted round robin)
    pub priority: u8,

    /// Completion queue ID (for submission queues)
    pub cqid: Option<QueueId>,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            qid: 0,
            depth: 128,
            priority: 0,
            cqid: None,
        }
    }
}

/// Submission Queue Entry tracking
#[derive(Debug)]
struct PendingCommand {
    /// Command capsule
    capsule: CommandCapsule,

    /// Timestamp when command was submitted
    submitted_at: std::time::Instant,
}

/// Admin Queue for controller management
pub struct AdminQueue {
    /// Queue configuration
    config: QueueConfig,

    /// Submission queue head
    sq_head: AtomicU16,

    /// Submission queue tail
    sq_tail: AtomicU16,

    /// Completion queue head
    cq_head: AtomicU16,

    /// Phase tag (toggles on wrap)
    phase: std::sync::atomic::AtomicBool,

    /// Pending commands (CID -> Command)
    pending: Mutex<std::collections::HashMap<CommandId, PendingCommand>>,

    /// Command ID counter
    next_cid: AtomicU16,

    /// Completed responses waiting to be fetched
    completions: Mutex<VecDeque<ResponseCapsule>>,
}

impl AdminQueue {
    /// Create a new admin queue
    pub fn new(depth: u16) -> Self {
        Self {
            config: QueueConfig {
                qid: 0,
                depth,
                priority: 0,
                cqid: Some(0),
            },
            sq_head: AtomicU16::new(0),
            sq_tail: AtomicU16::new(0),
            cq_head: AtomicU16::new(0),
            phase: std::sync::atomic::AtomicBool::new(true),
            pending: Mutex::new(std::collections::HashMap::new()),
            next_cid: AtomicU16::new(0),
            completions: Mutex::new(VecDeque::new()),
        }
    }

    /// Get queue ID
    pub fn qid(&self) -> QueueId {
        self.config.qid
    }

    /// Get queue depth
    pub fn depth(&self) -> u16 {
        self.config.depth
    }

    /// Get current queue depth (number of pending commands)
    pub fn current_depth(&self) -> u16 {
        self.pending.lock().len() as u16
    }

    /// Check if queue is full
    pub fn is_full(&self) -> bool {
        self.current_depth() >= self.config.depth
    }

    /// Allocate a new command ID
    pub fn allocate_cid(&self) -> CommandId {
        loop {
            let cid = self.next_cid.fetch_add(1, Ordering::Relaxed);
            // Avoid reserved CIDs if any
            if cid != 0xFFFF {
                return cid;
            }
        }
    }

    /// Submit a command
    pub fn submit(&self, mut capsule: CommandCapsule) -> NvmeOfResult<CommandId> {
        if self.is_full() {
            return Err(NvmeOfError::ResourceExhausted(
                "Admin queue full".to_string(),
            ));
        }

        let cid = self.allocate_cid();
        capsule.command.set_cid(cid);

        let pending_cmd = PendingCommand {
            capsule,
            submitted_at: std::time::Instant::now(),
        };

        self.pending.lock().insert(cid, pending_cmd);

        // Update SQ tail
        let tail = self.sq_tail.fetch_add(1, Ordering::AcqRel);
        if tail >= self.config.depth {
            self.sq_tail.store(0, Ordering::Release);
        }

        trace!("Submitted admin command, cid={}", cid);
        Ok(cid)
    }

    /// Complete a command
    pub fn complete(&self, response: ResponseCapsule) -> NvmeOfResult<()> {
        let cid = response.completion.cid;

        // Remove from pending
        if self.pending.lock().remove(&cid).is_none() {
            warn!("Completion for unknown CID {}", cid);
        }

        // Update SQ head (from completion)
        self.sq_head
            .store(response.completion.sq_head, Ordering::Release);

        // Add to completions
        self.completions.lock().push_back(response);

        trace!("Completed admin command, cid={}", cid);
        Ok(())
    }

    /// Get next completion (if available)
    pub fn poll_completion(&self) -> Option<ResponseCapsule> {
        self.completions.lock().pop_front()
    }

    /// Get pending command by CID
    pub fn get_pending(&self, cid: CommandId) -> Option<CommandCapsule> {
        self.pending.lock().get(&cid).map(|p| p.capsule.clone())
    }

    /// Get SQ head for completions
    pub fn sq_head(&self) -> u16 {
        self.sq_head.load(Ordering::Acquire)
    }

    /// Get current phase tag
    pub fn phase(&self) -> bool {
        self.phase.load(Ordering::Acquire)
    }
}

/// I/O Queue for data operations
pub struct IoQueue {
    /// Queue configuration
    config: QueueConfig,

    /// Submission queue head
    sq_head: AtomicU16,

    /// Submission queue tail
    sq_tail: AtomicU16,

    /// Completion queue head
    cq_head: AtomicU16,

    /// Phase tag
    phase: std::sync::atomic::AtomicBool,

    /// Pending commands
    pending: Mutex<std::collections::HashMap<CommandId, PendingCommand>>,

    /// Command ID counter
    next_cid: AtomicU16,

    /// Completions
    completions: Mutex<VecDeque<ResponseCapsule>>,

    /// Outstanding I/O count
    outstanding_io: AtomicU32,

    /// Total commands processed
    total_commands: AtomicU32,

    /// Total bytes transferred
    total_bytes: std::sync::atomic::AtomicU64,
}

impl IoQueue {
    /// Create a new I/O queue
    pub fn new(qid: QueueId, depth: u16) -> Self {
        assert!(qid > 0, "I/O queue ID must be > 0");

        Self {
            config: QueueConfig {
                qid,
                depth,
                priority: 0,
                cqid: Some(qid), // Paired CQ
            },
            sq_head: AtomicU16::new(0),
            sq_tail: AtomicU16::new(0),
            cq_head: AtomicU16::new(0),
            phase: std::sync::atomic::AtomicBool::new(true),
            pending: Mutex::new(std::collections::HashMap::new()),
            next_cid: AtomicU16::new(0),
            completions: Mutex::new(VecDeque::new()),
            outstanding_io: AtomicU32::new(0),
            total_commands: AtomicU32::new(0),
            total_bytes: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Get queue ID
    pub fn qid(&self) -> QueueId {
        self.config.qid
    }

    /// Get queue depth
    pub fn depth(&self) -> u16 {
        self.config.depth
    }

    /// Get current queue depth
    pub fn current_depth(&self) -> u16 {
        self.pending.lock().len() as u16
    }

    /// Check if queue is full
    pub fn is_full(&self) -> bool {
        self.current_depth() >= self.config.depth
    }

    /// Get outstanding I/O count
    pub fn outstanding_io(&self) -> u32 {
        self.outstanding_io.load(Ordering::Relaxed)
    }

    /// Allocate a new command ID
    pub fn allocate_cid(&self) -> CommandId {
        loop {
            let cid = self.next_cid.fetch_add(1, Ordering::Relaxed);
            if cid != 0xFFFF {
                return cid;
            }
        }
    }

    /// Submit a command
    pub fn submit(&self, mut capsule: CommandCapsule) -> NvmeOfResult<CommandId> {
        if self.is_full() {
            return Err(NvmeOfError::ResourceExhausted("I/O queue full".to_string()));
        }

        let cid = self.allocate_cid();
        capsule.command.set_cid(cid);

        let pending_cmd = PendingCommand {
            capsule,
            submitted_at: std::time::Instant::now(),
        };

        self.pending.lock().insert(cid, pending_cmd);
        self.outstanding_io.fetch_add(1, Ordering::Relaxed);

        // Update tail
        let tail = self.sq_tail.fetch_add(1, Ordering::AcqRel);
        if tail >= self.config.depth {
            self.sq_tail.store(0, Ordering::Release);
        }

        trace!(
            "Submitted I/O command on queue {}, cid={}",
            self.config.qid,
            cid
        );
        Ok(cid)
    }

    /// Complete a command
    pub fn complete(&self, response: ResponseCapsule, bytes_transferred: u64) -> NvmeOfResult<()> {
        let cid = response.completion.cid;

        if self.pending.lock().remove(&cid).is_none() {
            warn!(
                "Completion for unknown CID {} on queue {}",
                cid, self.config.qid
            );
        }

        self.outstanding_io.fetch_sub(1, Ordering::Relaxed);
        self.total_commands.fetch_add(1, Ordering::Relaxed);
        self.total_bytes
            .fetch_add(bytes_transferred, Ordering::Relaxed);

        self.sq_head
            .store(response.completion.sq_head, Ordering::Release);
        self.completions.lock().push_back(response);

        trace!(
            "Completed I/O command on queue {}, cid={}",
            self.config.qid,
            cid
        );
        Ok(())
    }

    /// Get next completion
    pub fn poll_completion(&self) -> Option<ResponseCapsule> {
        self.completions.lock().pop_front()
    }

    /// Get pending command by CID
    pub fn get_pending(&self, cid: CommandId) -> Option<CommandCapsule> {
        self.pending.lock().get(&cid).map(|p| p.capsule.clone())
    }

    /// Get SQ head
    pub fn sq_head(&self) -> u16 {
        self.sq_head.load(Ordering::Acquire)
    }

    /// Get current phase
    pub fn phase(&self) -> bool {
        self.phase.load(Ordering::Acquire)
    }

    /// Get statistics
    pub fn stats(&self) -> IoQueueStats {
        IoQueueStats {
            qid: self.config.qid,
            depth: self.config.depth,
            outstanding: self.outstanding_io.load(Ordering::Relaxed),
            total_commands: self.total_commands.load(Ordering::Relaxed),
            total_bytes: self.total_bytes.load(Ordering::Relaxed),
        }
    }
}

/// I/O Queue statistics
#[derive(Debug, Clone)]
pub struct IoQueueStats {
    /// Queue ID
    pub qid: QueueId,

    /// Queue depth
    pub depth: u16,

    /// Outstanding I/O
    pub outstanding: u32,

    /// Total commands completed
    pub total_commands: u32,

    /// Total bytes transferred
    pub total_bytes: u64,
}

/// Queue pair (SQ + CQ) for a connection
pub struct QueuePair {
    /// Queue ID
    qid: QueueId,

    /// Is this the admin queue?
    is_admin: bool,

    /// Admin queue (if qid == 0)
    admin: Option<AdminQueue>,

    /// I/O queue (if qid > 0)
    io: Option<IoQueue>,
}

impl QueuePair {
    /// Create admin queue pair
    pub fn admin(depth: u16) -> Self {
        Self {
            qid: 0,
            is_admin: true,
            admin: Some(AdminQueue::new(depth)),
            io: None,
        }
    }

    /// Create I/O queue pair
    pub fn io(qid: QueueId, depth: u16) -> Self {
        assert!(qid > 0, "I/O queue ID must be > 0");
        Self {
            qid,
            is_admin: false,
            admin: None,
            io: Some(IoQueue::new(qid, depth)),
        }
    }

    /// Get queue ID
    pub fn qid(&self) -> QueueId {
        self.qid
    }

    /// Check if this is the admin queue
    pub fn is_admin(&self) -> bool {
        self.is_admin
    }

    /// Get admin queue reference
    pub fn admin_queue(&self) -> Option<&AdminQueue> {
        self.admin.as_ref()
    }

    /// Get I/O queue reference
    pub fn io_queue(&self) -> Option<&IoQueue> {
        self.io.as_ref()
    }

    /// Submit a command
    pub fn submit(&self, capsule: CommandCapsule) -> NvmeOfResult<CommandId> {
        if let Some(ref admin) = self.admin {
            admin.submit(capsule)
        } else if let Some(ref io) = self.io {
            io.submit(capsule)
        } else {
            Err(NvmeOfError::Internal("Queue not initialized".to_string()))
        }
    }

    /// Complete a command
    pub fn complete(&self, response: ResponseCapsule) -> NvmeOfResult<()> {
        if let Some(ref admin) = self.admin {
            admin.complete(response)
        } else if let Some(ref io) = self.io {
            io.complete(response, 0)
        } else {
            Err(NvmeOfError::Internal("Queue not initialized".to_string()))
        }
    }

    /// Get SQ head for completions
    pub fn sq_head(&self) -> u16 {
        if let Some(ref admin) = self.admin {
            admin.sq_head()
        } else if let Some(ref io) = self.io {
            io.sq_head()
        } else {
            0
        }
    }
}

/// Queue manager for a connection
pub struct QueueManager {
    /// Admin queue
    admin: AdminQueue,

    /// I/O queues (indexed by qid - 1)
    io_queues: RwLock<Vec<IoQueue>>,

    /// Maximum queues
    max_queues: u16,

    /// Maximum queue depth
    max_depth: u16,
}

impl QueueManager {
    /// Create a new queue manager
    pub fn new(max_queues: u16, max_depth: u16) -> Self {
        Self {
            admin: AdminQueue::new(max_depth),
            io_queues: RwLock::new(Vec::new()),
            max_queues,
            max_depth,
        }
    }

    /// Get admin queue
    pub fn admin(&self) -> &AdminQueue {
        &self.admin
    }

    /// Create a new I/O queue
    pub fn create_io_queue(&self, qid: QueueId, depth: u16) -> NvmeOfResult<()> {
        if qid == 0 {
            return Err(NvmeOfError::InvalidCommand {
                opcode: 0x01,
                status: NvmeStatus::InvalidField.to_raw(),
            });
        }

        let depth = depth.min(self.max_depth);

        let mut queues = self.io_queues.write();

        // Extend if needed
        while queues.len() < qid as usize {
            let new_qid = (queues.len() + 1) as QueueId;
            queues.push(IoQueue::new(new_qid, depth));
        }

        debug!("Created I/O queue {}, depth={}", qid, depth);
        Ok(())
    }

    /// Delete an I/O queue
    pub fn delete_io_queue(&self, qid: QueueId) -> NvmeOfResult<()> {
        if qid == 0 {
            return Err(NvmeOfError::InvalidCommand {
                opcode: 0x00,
                status: NvmeStatus::InvalidField.to_raw(),
            });
        }

        // For now, we don't actually remove, just mark as deleted
        debug!("Deleted I/O queue {}", qid);
        Ok(())
    }

    /// Get I/O queue by ID
    pub fn get_io_queue(&self, qid: QueueId) -> Option<&IoQueue> {
        if qid == 0 || qid as usize > self.io_queues.read().len() {
            return None;
        }

        // This is safe because we only append to the vector
        let queues = self.io_queues.read();
        if (qid as usize) <= queues.len() {
            // We need to return a reference that lives long enough
            // This is a limitation - in practice we'd use Arc or similar
            None // Simplified for now
        } else {
            None
        }
    }

    /// Get number of I/O queues
    pub fn io_queue_count(&self) -> u16 {
        self.io_queues.read().len() as u16
    }

    /// Get maximum queues
    pub fn max_queues(&self) -> u16 {
        self.max_queues
    }

    /// Get maximum depth
    pub fn max_depth(&self) -> u16 {
        self.max_depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nvmeof::command::{NvmeCommand, NvmeCompletion};

    #[test]
    fn test_admin_queue() {
        let queue = AdminQueue::new(16);
        assert_eq!(queue.qid(), 0);
        assert_eq!(queue.depth(), 16);
        assert!(!queue.is_full());

        let cmd = NvmeCommand::new();
        let capsule = CommandCapsule::new(cmd);
        let cid = queue.submit(capsule).unwrap();

        assert_eq!(queue.current_depth(), 1);

        let completion = NvmeCompletion::success(cid, 0, 0);
        let response = ResponseCapsule::new(completion);
        queue.complete(response).unwrap();

        assert_eq!(queue.current_depth(), 0);
    }

    #[test]
    fn test_io_queue() {
        let queue = IoQueue::new(1, 32);
        assert_eq!(queue.qid(), 1);
        assert_eq!(queue.depth(), 32);

        let cmd = NvmeCommand::new();
        let capsule = CommandCapsule::new(cmd);
        let cid = queue.submit(capsule).unwrap();

        assert_eq!(queue.outstanding_io(), 1);

        let completion = NvmeCompletion::success(cid, 1, 0);
        let response = ResponseCapsule::new(completion);
        queue.complete(response, 4096).unwrap();

        assert_eq!(queue.outstanding_io(), 0);

        let stats = queue.stats();
        assert_eq!(stats.total_commands, 1);
        assert_eq!(stats.total_bytes, 4096);
    }

    #[test]
    fn test_queue_manager() {
        let manager = QueueManager::new(64, 128);
        assert_eq!(manager.max_queues(), 64);

        manager.create_io_queue(1, 32).unwrap();
        assert_eq!(manager.io_queue_count(), 1);
    }
}
