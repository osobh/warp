//! Shard data structures for erasure coding

/// Unique identifier for a shard
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShardId {
    /// Index within the shard set (0..total_shards)
    pub index: u16,
    /// Type of shard (data or parity)
    pub shard_type: ShardType,
}

impl ShardId {
    /// Create a new shard ID
    pub fn new(index: u16, shard_type: ShardType) -> Self {
        Self { index, shard_type }
    }

    /// Create a data shard ID
    pub fn data(index: u16) -> Self {
        Self::new(index, ShardType::Data)
    }

    /// Create a parity shard ID
    pub fn parity(index: u16) -> Self {
        Self::new(index, ShardType::Parity)
    }
}

/// Type of shard
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShardType {
    /// Original data shard
    Data,
    /// Parity/recovery shard
    Parity,
}

/// A shard with its identifier and data
#[derive(Debug, Clone)]
pub struct Shard {
    /// Shard identifier
    pub id: ShardId,
    /// Shard data
    pub data: Vec<u8>,
}

impl Shard {
    /// Create a new shard
    pub fn new(id: ShardId, data: Vec<u8>) -> Self {
        Self { id, data }
    }

    /// Create a data shard
    pub fn data(index: u16, data: Vec<u8>) -> Self {
        Self::new(ShardId::data(index), data)
    }

    /// Create a parity shard
    pub fn parity(index: u16, data: Vec<u8>) -> Self {
        Self::new(ShardId::parity(index), data)
    }

    /// Check if this is a data shard
    pub fn is_data(&self) -> bool {
        self.id.shard_type == ShardType::Data
    }

    /// Check if this is a parity shard
    pub fn is_parity(&self) -> bool {
        self.id.shard_type == ShardType::Parity
    }

    /// Get the shard size
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the shard is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shard_id() {
        let data_id = ShardId::data(5);
        assert_eq!(data_id.index, 5);
        assert_eq!(data_id.shard_type, ShardType::Data);

        let parity_id = ShardId::parity(2);
        assert_eq!(parity_id.index, 2);
        assert_eq!(parity_id.shard_type, ShardType::Parity);
    }

    #[test]
    fn test_shard() {
        let data = vec![1, 2, 3, 4];
        let shard = Shard::data(0, data.clone());

        assert!(shard.is_data());
        assert!(!shard.is_parity());
        assert_eq!(shard.len(), 4);
        assert_eq!(shard.data, data);
    }
}
