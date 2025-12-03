//! API types with OpenAPI schema definitions

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// Transfer request payload
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[schema(example = json!({
    "source": "/data/large-file.bin",
    "destination": "edge-node-01:/mnt/storage/large-file.bin",
    "compress": "zstd",
    "encrypt": true
}))]
pub struct TransferRequest {
    /// Source file path
    #[schema(example = "/data/large-file.bin")]
    pub source: String,

    /// Destination path (format: [edge-id:]path)
    #[schema(example = "edge-node-01:/mnt/storage/large-file.bin")]
    pub destination: String,

    /// Optional compression algorithm (zstd, lz4, none)
    #[schema(example = "zstd")]
    pub compress: Option<String>,

    /// Enable encryption
    #[schema(example = true)]
    pub encrypt: bool,
}

/// Transfer response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[schema(example = json!({
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "status": "InProgress",
    "created_at": 1701234567,
    "progress": 45.5
}))]
pub struct TransferResponse {
    /// Unique transfer identifier
    pub id: Uuid,

    /// Current transfer status
    pub status: TransferStatus,

    /// Creation timestamp (Unix epoch seconds)
    pub created_at: u64,

    /// Progress percentage (0.0 - 100.0)
    pub progress: f64,
}

/// Transfer status enumeration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum TransferStatus {
    /// Transfer is queued
    Pending,
    /// Transfer is in progress
    InProgress,
    /// Transfer completed successfully
    Completed,
    /// Transfer failed
    Failed,
    /// Transfer was cancelled
    Cancelled,
}

/// System information response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[schema(example = json!({
    "version": "0.1.0",
    "uptime_seconds": 3600,
    "capabilities": {
        "gpu_available": true,
        "max_transfers": 100,
        "supported_compression": ["zstd", "lz4", "none"],
        "supported_encryption": ["chacha20poly1305"]
    }
}))]
pub struct SystemInfo {
    /// API version
    pub version: String,

    /// System uptime in seconds
    pub uptime_seconds: u64,

    /// System capabilities
    pub capabilities: SystemCapabilities,
}

/// System capabilities
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct SystemCapabilities {
    /// GPU acceleration available
    pub gpu_available: bool,

    /// Maximum concurrent transfers
    pub max_transfers: usize,

    /// Supported compression algorithms
    pub supported_compression: Vec<String>,

    /// Supported encryption algorithms
    pub supported_encryption: Vec<String>,
}

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[schema(example = json!({
    "status": "healthy",
    "checks": {
        "database": "ok",
        "storage": "ok",
        "network": "ok"
    }
}))]
pub struct HealthStatus {
    /// Overall health status
    pub status: String,

    /// Individual component checks
    pub checks: std::collections::HashMap<String, String>,
}

/// Metrics response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[schema(example = json!({
    "active_transfers": 5,
    "total_bytes_transferred": 1073741824,
    "average_throughput_mbps": 125.5,
    "cpu_usage_percent": 45.2,
    "memory_usage_bytes": 536870912
}))]
pub struct MetricsResponse {
    /// Number of active transfers
    pub active_transfers: usize,

    /// Total bytes transferred since start
    pub total_bytes_transferred: u64,

    /// Average throughput in Mbps
    pub average_throughput_mbps: f64,

    /// CPU usage percentage
    pub cpu_usage_percent: f64,

    /// Memory usage in bytes
    pub memory_usage_bytes: u64,
}

/// Edge node information
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[schema(example = json!({
    "id": "edge-node-01",
    "address": "192.168.1.100:5000",
    "status": "online",
    "active_transfers": 3,
    "total_storage_bytes": 1099511627776_u64,
    "available_storage_bytes": 549755813888_u64
}))]
pub struct EdgeInfo {
    /// Edge node identifier
    pub id: String,

    /// Network address
    pub address: String,

    /// Node status
    pub status: String,

    /// Number of active transfers
    pub active_transfers: usize,

    /// Total storage capacity in bytes
    pub total_storage_bytes: u64,

    /// Available storage in bytes
    pub available_storage_bytes: u64,
}

/// Chunk information for a transfer
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[schema(example = json!({
    "chunk_id": 42,
    "offset": 4194304,
    "size": 1048576,
    "hash": "a1b2c3d4e5f6",
    "status": "completed"
}))]
pub struct ChunkInfo {
    /// Chunk identifier
    pub chunk_id: u64,

    /// Offset in file
    pub offset: u64,

    /// Chunk size in bytes
    pub size: u64,

    /// Chunk hash (hex encoded)
    pub hash: String,

    /// Chunk transfer status
    pub status: String,
}

/// Transfer list response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct TransferListResponse {
    /// List of transfers
    pub transfers: Vec<TransferResponse>,

    /// Total count
    pub total: usize,
}

/// Edge list response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct EdgeListResponse {
    /// List of edge nodes
    pub edges: Vec<EdgeInfo>,

    /// Total count
    pub total: usize,
}

/// Error response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct ErrorResponse {
    /// Error message
    pub error: String,

    /// HTTP status code
    pub status: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_request_serialization() {
        let req = TransferRequest {
            source: "/data/file.bin".to_string(),
            destination: "edge-01:/mnt/file.bin".to_string(),
            compress: Some("zstd".to_string()),
            encrypt: true,
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"source\":\"/data/file.bin\""));
        assert!(json.contains("\"compress\":\"zstd\""));
        assert!(json.contains("\"encrypt\":true"));
    }

    #[test]
    fn test_transfer_request_deserialization() {
        let json = r#"{
            "source": "/data/test.bin",
            "destination": "edge-02:/storage/test.bin",
            "compress": "lz4",
            "encrypt": false
        }"#;

        let req: TransferRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.source, "/data/test.bin");
        assert_eq!(req.destination, "edge-02:/storage/test.bin");
        assert_eq!(req.compress, Some("lz4".to_string()));
        assert!(!req.encrypt);
    }

    #[test]
    fn test_transfer_request_optional_compress() {
        let json = r#"{
            "source": "/data/test.bin",
            "destination": "edge-02:/storage/test.bin",
            "compress": null,
            "encrypt": false
        }"#;

        let req: TransferRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.compress, None);
    }

    #[test]
    fn test_transfer_response_serialization() {
        let id = Uuid::new_v4();
        let resp = TransferResponse {
            id,
            status: TransferStatus::InProgress,
            created_at: 1701234567,
            progress: 45.5,
        };

        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(&id.to_string()));
        assert!(json.contains("\"status\":\"InProgress\""));
        assert!(json.contains("\"progress\":45.5"));
    }

    #[test]
    fn test_transfer_status_serialization() {
        let statuses = vec![
            TransferStatus::Pending,
            TransferStatus::InProgress,
            TransferStatus::Completed,
            TransferStatus::Failed,
            TransferStatus::Cancelled,
        ];

        let expected = vec!["Pending", "InProgress", "Completed", "Failed", "Cancelled"];

        for (status, expected_str) in statuses.iter().zip(expected.iter()) {
            let json = serde_json::to_string(status).unwrap();
            assert_eq!(json, format!("\"{}\"", expected_str));
        }
    }

    #[test]
    fn test_transfer_status_deserialization() {
        let json = "\"InProgress\"";
        let status: TransferStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status, TransferStatus::InProgress);
    }

    #[test]
    fn test_system_info_serialization() {
        let info = SystemInfo {
            version: "0.1.0".to_string(),
            uptime_seconds: 3600,
            capabilities: SystemCapabilities {
                gpu_available: true,
                max_transfers: 100,
                supported_compression: vec!["zstd".to_string(), "lz4".to_string()],
                supported_encryption: vec!["chacha20poly1305".to_string()],
            },
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"version\":\"0.1.0\""));
        assert!(json.contains("\"gpu_available\":true"));
        assert!(json.contains("\"max_transfers\":100"));
    }

    #[test]
    fn test_system_capabilities_deserialization() {
        let json = r#"{
            "gpu_available": false,
            "max_transfers": 50,
            "supported_compression": ["zstd"],
            "supported_encryption": ["chacha20poly1305"]
        }"#;

        let caps: SystemCapabilities = serde_json::from_str(json).unwrap();
        assert!(!caps.gpu_available);
        assert_eq!(caps.max_transfers, 50);
        assert_eq!(caps.supported_compression.len(), 1);
    }

    #[test]
    fn test_health_status_serialization() {
        let mut checks = std::collections::HashMap::new();
        checks.insert("database".to_string(), "ok".to_string());
        checks.insert("storage".to_string(), "ok".to_string());

        let health = HealthStatus {
            status: "healthy".to_string(),
            checks,
        };

        let json = serde_json::to_string(&health).unwrap();
        assert!(json.contains("\"status\":\"healthy\""));
        assert!(json.contains("\"database\""));
    }

    #[test]
    fn test_metrics_response_serialization() {
        let metrics = MetricsResponse {
            active_transfers: 5,
            total_bytes_transferred: 1_073_741_824,
            average_throughput_mbps: 125.5,
            cpu_usage_percent: 45.2,
            memory_usage_bytes: 536_870_912,
        };

        let json = serde_json::to_string(&metrics).unwrap();
        assert!(json.contains("\"active_transfers\":5"));
        assert!(json.contains("\"total_bytes_transferred\":1073741824"));
    }

    #[test]
    fn test_edge_info_serialization() {
        let edge = EdgeInfo {
            id: "edge-01".to_string(),
            address: "192.168.1.100:5000".to_string(),
            status: "online".to_string(),
            active_transfers: 3,
            total_storage_bytes: 1_099_511_627_776,
            available_storage_bytes: 549_755_813_888,
        };

        let json = serde_json::to_string(&edge).unwrap();
        assert!(json.contains("\"id\":\"edge-01\""));
        assert!(json.contains("\"status\":\"online\""));
    }

    #[test]
    fn test_chunk_info_serialization() {
        let chunk = ChunkInfo {
            chunk_id: 42,
            offset: 4_194_304,
            size: 1_048_576,
            hash: "a1b2c3d4e5f6".to_string(),
            status: "completed".to_string(),
        };

        let json = serde_json::to_string(&chunk).unwrap();
        assert!(json.contains("\"chunk_id\":42"));
        assert!(json.contains("\"hash\":\"a1b2c3d4e5f6\""));
    }

    #[test]
    fn test_transfer_list_response_serialization() {
        let transfers = vec![
            TransferResponse {
                id: Uuid::new_v4(),
                status: TransferStatus::Completed,
                created_at: 1701234567,
                progress: 100.0,
            },
            TransferResponse {
                id: Uuid::new_v4(),
                status: TransferStatus::InProgress,
                created_at: 1701234600,
                progress: 50.0,
            },
        ];

        let response = TransferListResponse {
            transfers: transfers.clone(),
            total: transfers.len(),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"total\":2"));
        assert!(json.contains("\"transfers\""));
    }

    #[test]
    fn test_edge_list_response_serialization() {
        let edges = vec![
            EdgeInfo {
                id: "edge-01".to_string(),
                address: "192.168.1.100:5000".to_string(),
                status: "online".to_string(),
                active_transfers: 3,
                total_storage_bytes: 1_000_000_000,
                available_storage_bytes: 500_000_000,
            },
        ];

        let response = EdgeListResponse {
            edges: edges.clone(),
            total: edges.len(),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"total\":1"));
        assert!(json.contains("\"edges\""));
    }

    #[test]
    fn test_error_response_serialization() {
        let error = ErrorResponse {
            error: "Transfer not found".to_string(),
            status: 404,
        };

        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"error\":\"Transfer not found\""));
        assert!(json.contains("\"status\":404"));
    }

    #[test]
    fn test_transfer_request_equality() {
        let req1 = TransferRequest {
            source: "/data/file.bin".to_string(),
            destination: "edge-01:/mnt/file.bin".to_string(),
            compress: Some("zstd".to_string()),
            encrypt: true,
        };

        let req2 = req1.clone();
        assert_eq!(req1, req2);
    }

    #[test]
    fn test_transfer_status_equality() {
        assert_eq!(TransferStatus::Pending, TransferStatus::Pending);
        assert_ne!(TransferStatus::Pending, TransferStatus::Completed);
    }
}
