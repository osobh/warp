//! Tests for the orchestrator module

use super::*;

fn test_transfer_request(
    direction: TransferDirection,
    num_chunks: usize,
) -> TransferRequest {
    let chunks = (0..num_chunks)
        .map(|i| {
            let mut hash = [0u8; 32];
            hash[0] = i as u8;
            hash
        })
        .collect();
    let chunk_sizes = vec![1024; num_chunks];
    TransferRequest::new(chunks, chunk_sizes, direction)
}

fn test_sources(num_chunks: usize) -> HashMap<ChunkId, Vec<EdgeIdx>> {
    let mut sources = HashMap::new();
    for i in 0..num_chunks {
        let mut hash = [0u8; 32];
        hash[0] = i as u8;
        let chunk_id = ChunkId::from_hash(&hash);
        sources.insert(chunk_id, vec![EdgeIdx::new(0), EdgeIdx::new(1)]);
    }
    sources
}

#[test]
fn test_orchestrator_config_default() {
    let config = OrchestratorConfig::default();
    assert_eq!(config.tick_interval_ms, 100);
    assert_eq!(config.max_concurrent_transfers, 10);
    assert!(config.validate().is_ok());
}

#[test]
fn test_orchestrator_config_new() {
    let config = OrchestratorConfig::new();
    assert_eq!(config.tick_interval_ms, 100);
    assert_eq!(config.max_concurrent_transfers, 10);
}

#[test]
fn test_orchestrator_config_validation() {
    let valid = OrchestratorConfig::default();
    assert!(valid.validate().is_ok());

    let invalid = OrchestratorConfig {
        tick_interval_ms: 0,
        ..Default::default()
    };
    assert!(invalid.validate().is_err());

    let invalid = OrchestratorConfig {
        max_concurrent_transfers: 0,
        ..Default::default()
    };
    assert!(invalid.validate().is_err());

    let invalid = OrchestratorConfig {
        pool_config: PoolConfig {
            max_connections_per_edge: 0,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(invalid.validate().is_err());
}

#[test]
fn test_orchestrator_config_builders() {
    let config = OrchestratorConfig::new()
        .with_tick_interval(200)
        .with_max_concurrent_transfers(5);

    assert_eq!(config.tick_interval_ms, 200);
    assert_eq!(config.max_concurrent_transfers, 5);
}

#[test]
fn test_transfer_handle_creation() {
    let handle = TransferHandle::new(TransferId(42), TransferDirection::Download, 1000);
    assert_eq!(handle.id, TransferId(42));
    assert_eq!(handle.direction, TransferDirection::Download);
    assert_eq!(handle.status, TransferStatus::Pending);
    assert_eq!(handle.created_at_ms, 1000);
}

#[test]
fn test_transfer_handle_elapsed() {
    let created_at = current_time_ms() - 500;
    let handle = TransferHandle::new(TransferId(1), TransferDirection::Download, created_at);
    let elapsed = handle.elapsed_ms();
    assert!(elapsed >= 500);
}

#[test]
fn test_orchestrator_new() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config);
    assert!(orch.is_ok());
}

#[test]
fn test_orchestrator_new_invalid_config() {
    let config = OrchestratorConfig {
        tick_interval_ms: 0,
        ..Default::default()
    };
    let orch = Orchestrator::new(config);
    assert!(orch.is_err());
}

#[tokio::test]
async fn test_download_request() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Download, 5);
    let sources = test_sources(5);

    let handle = orch.download(request, sources).await;
    assert!(handle.is_ok());

    let handle = handle.unwrap();
    assert_eq!(handle.direction, TransferDirection::Download);
    assert_eq!(handle.status, TransferStatus::Pending);
}

#[tokio::test]
async fn test_upload_request() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Upload, 3);
    let destinations = test_sources(3);

    let handle = orch.upload(request, destinations).await;
    assert!(handle.is_ok());

    let handle = handle.unwrap();
    assert_eq!(handle.direction, TransferDirection::Upload);
    assert_eq!(handle.status, TransferStatus::Pending);
}

#[tokio::test]
async fn test_download_invalid_direction() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Upload, 5);
    let sources = test_sources(5);

    let result = orch.download(request, sources).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_upload_invalid_direction() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Download, 3);
    let destinations = test_sources(3);

    let result = orch.upload(request, destinations).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_download_no_sources() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Download, 5);
    let sources = HashMap::new();

    let result = orch.download(request, sources).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        OrchError::NoSources => {}
        _ => panic!("Expected NoSources error"),
    }
}

#[tokio::test]
async fn test_upload_no_destinations() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Upload, 3);
    let destinations = HashMap::new();

    let result = orch.upload(request, destinations).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        OrchError::NoSources => {}
        _ => panic!("Expected NoSources error"),
    }
}

#[tokio::test]
async fn test_queue_upload_chunk() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Upload, 2);
    let destinations = test_sources(2);

    let handle = orch.upload(request.clone(), destinations).await.unwrap();

    let chunk_id = ChunkId::from_hash(&request.chunks[0]);
    let data = vec![0u8; 1024];

    let result = orch.queue_upload_chunk(handle.id, chunk_id, data);
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_queue_upload_chunk_wrong_size() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Upload, 2);
    let destinations = test_sources(2);

    let handle = orch.upload(request.clone(), destinations).await.unwrap();

    let chunk_id = ChunkId::from_hash(&request.chunks[0]);
    let data = vec![0u8; 500]; // Wrong size

    let result = orch.queue_upload_chunk(handle.id, chunk_id, data);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_queue_upload_chunk_not_found() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let chunk_id = ChunkId::from_hash(&[0u8; 32]);
    let data = vec![0u8; 1024];

    let result = orch.queue_upload_chunk(TransferId(999), chunk_id, data);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_tick_no_transfers() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let results = orch.tick().await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_tick_with_download() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Download, 3);
    let sources = test_sources(3);

    let handle = orch.download(request, sources).await.unwrap();

    // First tick should complete the download
    let results = orch.tick().await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, handle.id);
    assert!(results[0].success);
    assert_eq!(results[0].chunks_transferred, 3);

    // Second tick should have no more transfers
    let results = orch.tick().await.unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_tick_with_upload() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Upload, 2);
    let destinations = test_sources(2);

    let handle = orch.upload(request.clone(), destinations).await.unwrap();

    // Queue all chunks
    for (i, chunk_hash) in request.chunks.iter().enumerate() {
        let chunk_id = ChunkId::from_hash(chunk_hash);
        let data = vec![i as u8; 1024];
        orch.queue_upload_chunk(handle.id, chunk_id, data).unwrap();
    }

    // Tick should complete the upload
    let results = orch.tick().await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, handle.id);
    assert!(results[0].success);
    assert_eq!(results[0].chunks_transferred, 2);
}

#[tokio::test]
async fn test_tick_multiple_transfers() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    // Create multiple downloads
    let request1 = test_transfer_request(TransferDirection::Download, 2);
    let sources1 = test_sources(2);
    let _handle1 = orch.download(request1, sources1).await.unwrap();

    let mut sources2 = HashMap::new();
    // Create offset chunk hashes
    for i in 0..3 {
        let mut hash = [0u8; 32];
        hash[0] = (i + 10) as u8;
        let chunk_id = ChunkId::from_hash(&hash);
        sources2.insert(chunk_id, vec![EdgeIdx::new(0), EdgeIdx::new(1)]);
    }

    let request2 = {
        let chunks = (0..3)
            .map(|i| {
                let mut hash = [0u8; 32];
                hash[0] = (i + 10) as u8;
                hash
            })
            .collect();
        let chunk_sizes = vec![1024; 3];
        TransferRequest::new(chunks, chunk_sizes, TransferDirection::Download)
    };
    let _handle2 = orch.download(request2, sources2).await.unwrap();

    // Tick should complete both
    let results = orch.tick().await.unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.success));
}

#[tokio::test]
async fn test_status_existing_transfer() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Download, 2);
    let sources = test_sources(2);

    let handle = orch.download(request, sources).await.unwrap();

    let status = orch.status(handle.id);
    assert!(status.is_some());
    assert_eq!(status.unwrap(), TransferStatus::Pending);
}

#[tokio::test]
async fn test_status_nonexistent_transfer() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let status = orch.status(TransferId(999));
    assert!(status.is_none());
}

#[tokio::test]
async fn test_progress_tracking() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Download, 5);
    let sources = test_sources(5);

    let handle = orch.download(request, sources).await.unwrap();

    let progress = orch.progress(handle.id);
    assert!(progress.is_some());

    let progress = progress.unwrap();
    assert_eq!(progress.transfer_id, handle.id);
    assert_eq!(progress.total_chunks, 5);
    assert_eq!(progress.total_bytes, 5120);
}

#[tokio::test]
async fn test_cancel_download() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Download, 3);
    let sources = test_sources(3);

    let handle = orch.download(request, sources).await.unwrap();

    let result = orch.cancel(handle.id);
    assert!(result.is_ok());

    let status = orch.status(handle.id);
    assert!(status.is_some());
    assert_eq!(status.unwrap(), TransferStatus::Cancelled);
}

#[tokio::test]
async fn test_cancel_upload() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Upload, 2);
    let destinations = test_sources(2);

    let handle = orch.upload(request, destinations).await.unwrap();

    let result = orch.cancel(handle.id);
    assert!(result.is_ok());

    let status = orch.status(handle.id);
    assert!(status.is_some());
    assert_eq!(status.unwrap(), TransferStatus::Cancelled);
}

#[tokio::test]
async fn test_cancel_nonexistent() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let result = orch.cancel(TransferId(999));
    assert!(result.is_err());
}

#[tokio::test]
async fn test_active_transfers_list() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    assert!(orch.active_transfers().is_empty());

    let request1 = test_transfer_request(TransferDirection::Download, 2);
    let sources1 = test_sources(2);
    let _handle1 = orch.download(request1, sources1).await.unwrap();

    let mut sources2 = HashMap::new();
    for i in 0..3 {
        let mut hash = [0u8; 32];
        hash[0] = (i + 10) as u8;
        let chunk_id = ChunkId::from_hash(&hash);
        sources2.insert(chunk_id, vec![EdgeIdx::new(0)]);
    }
    let request2 = {
        let chunks = (0..3)
            .map(|i| {
                let mut hash = [0u8; 32];
                hash[0] = (i + 10) as u8;
                hash
            })
            .collect();
        let chunk_sizes = vec![1024; 3];
        TransferRequest::new(chunks, chunk_sizes, TransferDirection::Upload)
    };
    let _handle2 = orch.upload(request2, sources2).await.unwrap();

    let active = orch.active_transfers();
    assert_eq!(active.len(), 2);
    assert!(active.iter().any(|h| h.direction == TransferDirection::Download));
    assert!(active.iter().any(|h| h.direction == TransferDirection::Upload));
}

#[tokio::test]
async fn test_subscribe_progress() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let mut rx = orch.subscribe();

    let request = test_transfer_request(TransferDirection::Download, 2);
    let sources = test_sources(2);

    let handle = orch.download(request, sources).await.unwrap();

    // Tick to start the transfer
    let _ = orch.tick().await;

    // Should receive progress updates
    let timeout_result = tokio::time::timeout(
        tokio::time::Duration::from_millis(100),
        rx.recv(),
    )
    .await;

    assert!(timeout_result.is_ok());
    if let Ok(Ok(update)) = timeout_result {
        assert_eq!(update.transfer_id, handle.id);
    }
}

#[tokio::test]
async fn test_shutdown() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request1 = test_transfer_request(TransferDirection::Download, 2);
    let sources1 = test_sources(2);
    let handle1 = orch.download(request1, sources1).await.unwrap();

    let mut sources2 = HashMap::new();
    for i in 0..3 {
        let mut hash = [0u8; 32];
        hash[0] = (i + 10) as u8;
        sources2.insert(ChunkId::from_hash(&hash), vec![EdgeIdx::new(0)]);
    }
    let request2 = {
        let chunks = (0..3)
            .map(|i| {
                let mut hash = [0u8; 32];
                hash[0] = (i + 10) as u8;
                hash
            })
            .collect();
        let chunk_sizes = vec![1024; 3];
        TransferRequest::new(chunks, chunk_sizes, TransferDirection::Upload)
    };
    let handle2 = orch.upload(request2, sources2).await.unwrap();

    assert_eq!(orch.active_transfers().len(), 2);

    orch.shutdown().await;

    assert!(orch.active_transfers().is_empty());

    let status1 = orch.status(handle1.id).unwrap();
    let status2 = orch.status(handle2.id).unwrap();

    assert_eq!(status1, TransferStatus::Cancelled);
    assert_eq!(status2, TransferStatus::Cancelled);
}

#[tokio::test]
async fn test_max_concurrent_transfers() {
    let config = OrchestratorConfig::default().with_max_concurrent_transfers(2);
    let orch = Orchestrator::new(config).unwrap();

    let request1 = test_transfer_request(TransferDirection::Download, 2);
    let sources1 = test_sources(2);
    let _handle1 = orch.download(request1, sources1).await.unwrap();

    let mut sources2 = HashMap::new();
    sources2.insert(
        ChunkId::from_hash(&[10u8; 32]),
        vec![EdgeIdx::new(0)],
    );
    let request2 = {
        let chunks = vec![[10u8; 32]];
        let chunk_sizes = vec![1024];
        TransferRequest::new(chunks, chunk_sizes, TransferDirection::Upload)
    };
    let _handle2 = orch.upload(request2, sources2).await.unwrap();

    // Third transfer should fail
    let mut sources3 = HashMap::new();
    sources3.insert(
        ChunkId::from_hash(&[20u8; 32]),
        vec![EdgeIdx::new(0)],
    );
    let request3 = {
        let chunks = vec![[20u8; 32]];
        let chunk_sizes = vec![1024];
        TransferRequest::new(chunks, chunk_sizes, TransferDirection::Download)
    };
    let result = orch.download(request3, sources3).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_transfer_id_generation() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request1 = test_transfer_request(TransferDirection::Download, 1);
    let sources1 = test_sources(1);
    let handle1 = orch.download(request1, sources1).await.unwrap();

    let mut sources2 = HashMap::new();
    sources2.insert(
        ChunkId::from_hash(&[10u8; 32]),
        vec![EdgeIdx::new(0)],
    );
    let request2 = {
        let chunks = vec![[10u8; 32]];
        let chunk_sizes = vec![1024];
        TransferRequest::new(chunks, chunk_sizes, TransferDirection::Download)
    };
    let handle2 = orch.download(request2, sources2).await.unwrap();

    assert_ne!(handle1.id, handle2.id);
    assert_eq!(handle1.id.as_u64() + 1, handle2.id.as_u64());
}

#[tokio::test]
async fn test_completed_transfers_removed() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Download, 2);
    let sources = test_sources(2);

    let handle = orch.download(request, sources).await.unwrap();

    assert_eq!(orch.active_transfers().len(), 1);

    // Complete the transfer
    let results = orch.tick().await.unwrap();
    assert_eq!(results.len(), 1);

    // Transfer should be removed from active list
    assert!(orch.active_transfers().is_empty());

    // But status should still be available from progress tracker
    let status = orch.status(handle.id);
    assert!(status.is_some());
    assert_eq!(status.unwrap(), TransferStatus::Completed);
}

#[test]
fn test_orchestrator_pool_access() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let pool = orch.pool();
    assert!(pool.stats().total_connections == 0);
}

#[test]
fn test_orchestrator_config_access() {
    let config = OrchestratorConfig::default().with_tick_interval(250);
    let orch = Orchestrator::new(config).unwrap();

    assert_eq!(orch.config().tick_interval_ms, 250);
}

#[tokio::test]
async fn test_upload_partial_chunks_queued() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Upload, 3);
    let destinations = test_sources(3);

    let handle = orch.upload(request.clone(), destinations).await.unwrap();

    // Queue only 2 of 3 chunks
    for i in 0..2 {
        let chunk_id = ChunkId::from_hash(&request.chunks[i]);
        let data = vec![i as u8; 1024];
        orch.queue_upload_chunk(handle.id, chunk_id, data).unwrap();
    }

    // Tick should not complete
    let results = orch.tick().await.unwrap();
    assert!(results.is_empty());

    assert_eq!(orch.active_transfers().len(), 1);
}

#[tokio::test]
async fn test_multiple_ticks_no_double_completion() {
    let config = OrchestratorConfig::default();
    let orch = Orchestrator::new(config).unwrap();

    let request = test_transfer_request(TransferDirection::Download, 2);
    let sources = test_sources(2);

    let _handle = orch.download(request, sources).await.unwrap();

    // First tick completes
    let results1 = orch.tick().await.unwrap();
    assert_eq!(results1.len(), 1);

    // Second tick should not return the same transfer
    let results2 = orch.tick().await.unwrap();
    assert!(results2.is_empty());

    // Third tick should still be empty
    let results3 = orch.tick().await.unwrap();
    assert!(results3.is_empty());
}
