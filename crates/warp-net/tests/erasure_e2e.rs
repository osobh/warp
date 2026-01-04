//! End-to-end tests for erasure coding over QUIC transport
//!
//! These tests verify the complete erasure coding pipeline:
//! - Encode data into shards
//! - Send shards over QUIC using Frame::Shard
//! - Receive and buffer shards
//! - Decode shards back into original data
//!
//! Run with: cargo test -p warp-net --test erasure_e2e --features insecure-tls

#![cfg(feature = "insecure-tls")]

use bytes::Bytes;
use std::collections::HashMap;
use warp_ec::{ErasureConfig, ErasureDecoder, ErasureEncoder};
use warp_net::{Frame, WarpEndpoint};

/// Buffer for collecting shards before decoding
struct ShardBuffer {
    total_shards: usize,
    data_shards: usize,
    shards: Vec<Option<Vec<u8>>>,
    received_count: usize,
}

impl ShardBuffer {
    fn new(data_shards: usize, parity_shards: usize) -> Self {
        let total = data_shards + parity_shards;
        Self {
            total_shards: total,
            data_shards,
            shards: vec![None; total],
            received_count: 0,
        }
    }

    fn insert(&mut self, shard_idx: u16, data: Vec<u8>) {
        let idx = shard_idx as usize;
        if idx < self.total_shards && self.shards[idx].is_none() {
            self.shards[idx] = Some(data);
            self.received_count += 1;
        }
    }

    fn can_decode(&self) -> bool {
        self.received_count >= self.data_shards
    }

    fn take_shards(&mut self) -> Vec<Option<Vec<u8>>> {
        std::mem::replace(&mut self.shards, vec![None; self.total_shards])
    }
}

/// Test erasure coding encode/decode without network (always passes)
#[test]
fn test_erasure_encode_decode_unit() {
    let data_shards = 4;
    let parity_shards = 2;
    let config = ErasureConfig::new(data_shards, parity_shards).unwrap();
    let encoder = ErasureEncoder::new(config.clone());
    let decoder = ErasureDecoder::new(config);

    // Original data
    let original_data: Vec<u8> = (0..256).map(|i| i as u8).collect();

    // Encode
    let shards = encoder.encode(&original_data).unwrap();
    assert_eq!(shards.len(), 6);

    // Simulate receiving all shards
    let received_shards: Vec<Option<Vec<u8>>> = shards.into_iter().map(Some).collect();

    // Decode
    let decoded = decoder.decode(&received_shards).unwrap();
    assert_eq!(decoded[..original_data.len()], original_data[..]);
}

/// Test erasure coding with simulated loss (without network)
#[test]
fn test_erasure_recovery_unit() {
    let data_shards = 4;
    let parity_shards = 2;
    let config = ErasureConfig::new(data_shards, parity_shards).unwrap();
    let encoder = ErasureEncoder::new(config.clone());
    let decoder = ErasureDecoder::new(config);

    // Original data
    let original_data: Vec<u8> = (0..128).map(|i| (i * 2) as u8).collect();

    // Encode
    let shards = encoder.encode(&original_data).unwrap();

    // Simulate loss: only receive shards 0, 1, 4, 5 (lose 2 and 3)
    let received_shards: Vec<Option<Vec<u8>>> = vec![
        Some(shards[0].clone()),
        Some(shards[1].clone()),
        None, // lost
        None, // lost
        Some(shards[4].clone()),
        Some(shards[5].clone()),
    ];

    // Decode should still work
    let decoded = decoder.decode(&received_shards).unwrap();
    assert_eq!(decoded[..original_data.len()], original_data[..]);
}

/// Test that decode fails with insufficient shards (without network)
#[test]
fn test_erasure_insufficient_shards_unit() {
    let data_shards = 4;
    let parity_shards = 2;
    let config = ErasureConfig::new(data_shards, parity_shards).unwrap();
    let encoder = ErasureEncoder::new(config.clone());
    let decoder = ErasureDecoder::new(config);

    // Original data
    let original_data: Vec<u8> = (0..64).collect();

    // Encode
    let shards = encoder.encode(&original_data).unwrap();

    // Only 3 shards (need at least 4)
    let received_shards: Vec<Option<Vec<u8>>> = vec![
        Some(shards[0].clone()),
        Some(shards[1].clone()),
        None,
        None,
        None,
        Some(shards[5].clone()),
    ];

    // Decode should fail
    let result = decoder.decode(&received_shards);
    assert!(result.is_err());
}

/// Test basic erasure coding roundtrip: encode -> send -> receive -> decode -> verify
#[tokio::test]
#[ignore = "flaky network test - QUIC connection timing issues"]
async fn test_erasure_e2e_basic() {
    // Configuration: 4 data shards + 2 parity = 6 total
    let data_shards = 4;
    let parity_shards = 2;
    let config = ErasureConfig::new(data_shards, parity_shards).unwrap();
    let encoder = ErasureEncoder::new(config.clone());
    let decoder = ErasureDecoder::new(config);

    // Original data (must be divisible by data_shards for simplicity)
    let original_data: Vec<u8> = (0..256).map(|i| i as u8).collect();
    let original_len = original_data.len();

    // Encode into shards
    let shards = encoder.encode(&original_data).unwrap();
    assert_eq!(shards.len(), 6);

    // Start server using WarpEndpoint::server (same as internal tests)
    let server = WarpEndpoint::server("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = server.local_addr().unwrap();

    // Server task: receive shards and decode
    let server_task = tokio::spawn(async move {
        let conn = server.accept().await.unwrap();
        conn.handshake_server().await.unwrap();

        let mut shard_buffer = ShardBuffer::new(data_shards, parity_shards);

        // Receive all shards
        for _ in 0..6 {
            let frame = conn.recv_frame().await.unwrap();
            match frame {
                Frame::Shard {
                    chunk_id: _,
                    shard_idx,
                    total_shards,
                    data,
                } => {
                    assert_eq!(total_shards, 6);
                    shard_buffer.insert(shard_idx, data.to_vec());
                }
                _ => panic!("Expected Shard frame"),
            }
        }

        // Should be able to decode now
        assert!(shard_buffer.can_decode());
        let shards_for_decode = shard_buffer.take_shards();
        let decoded = decoder.decode(&shards_for_decode).unwrap();

        // Truncate to original length (padding was added)
        decoded[..original_len].to_vec()
    });

    // Small delay to ensure server is ready
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Client: connect and send shards
    let client = WarpEndpoint::client().await.unwrap();
    let conn = client.connect(addr, "localhost").await.unwrap();
    conn.handshake().await.unwrap();

    // Send all shards
    for (idx, shard) in shards.iter().enumerate() {
        conn.send_frame(Frame::Shard {
            chunk_id: 0,
            shard_idx: idx as u16,
            total_shards: 6,
            data: Bytes::from(shard.clone()),
        })
        .await
        .unwrap();
    }

    // Keep connection alive while server processes
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Wait for server to decode and verify
    let decoded = server_task.await.unwrap();
    assert_eq!(decoded, original_data);
}

/// Test erasure coding with shard loss: send only 4 of 6 shards, verify recovery works
#[tokio::test]
#[ignore = "flaky network test - QUIC connection timing issues"]
async fn test_erasure_e2e_with_loss() {
    let data_shards = 4;
    let parity_shards = 2;
    let config = ErasureConfig::new(data_shards, parity_shards).unwrap();
    let encoder = ErasureEncoder::new(config.clone());
    let decoder = ErasureDecoder::new(config);

    // Original data
    let original_data: Vec<u8> = (0..128).map(|i| (i * 2) as u8).collect();
    let original_len = original_data.len();

    // Encode into shards
    let shards = encoder.encode(&original_data).unwrap();

    // Simulate loss: only send shards 0, 1, 4, 5 (skip 2 and 3)
    // This means we lose 2 data shards but have 2 parity shards to compensate
    let shards_to_send: Vec<(usize, &Vec<u8>)> = vec![
        (0, &shards[0]),
        (1, &shards[1]),
        (4, &shards[4]), // parity
        (5, &shards[5]), // parity
    ];

    let server = WarpEndpoint::server("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = server.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        let conn = server.accept().await.unwrap();
        conn.handshake_server().await.unwrap();

        let mut shard_buffer = ShardBuffer::new(data_shards, parity_shards);

        // Receive only 4 shards
        for _ in 0..4 {
            let frame = conn.recv_frame().await.unwrap();
            if let Frame::Shard {
                shard_idx, data, ..
            } = frame
            {
                shard_buffer.insert(shard_idx, data.to_vec());
            }
        }

        // Should still be able to decode with exactly data_shards available
        assert!(shard_buffer.can_decode());
        let shards_for_decode = shard_buffer.take_shards();
        let decoded = decoder.decode(&shards_for_decode).unwrap();
        decoded[..original_len].to_vec()
    });

    // Small delay to ensure server is ready
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let client = WarpEndpoint::client().await.unwrap();
    let conn = client.connect(addr, "localhost").await.unwrap();
    conn.handshake().await.unwrap();

    // Send only 4 shards (simulating 2 lost)
    for (idx, shard) in shards_to_send {
        conn.send_frame(Frame::Shard {
            chunk_id: 0,
            shard_idx: idx as u16,
            total_shards: 6,
            data: Bytes::from(shard.clone()),
        })
        .await
        .unwrap();
    }

    // Keep connection alive while server processes
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let decoded = server_task.await.unwrap();
    assert_eq!(decoded, original_data);
}

/// Test that decode fails gracefully when too many shards are lost
#[tokio::test]
#[ignore = "flaky network test - QUIC connection timing issues"]
async fn test_erasure_e2e_too_many_losses() {
    let data_shards = 4;
    let parity_shards = 2;
    let config = ErasureConfig::new(data_shards, parity_shards).unwrap();
    let encoder = ErasureEncoder::new(config.clone());
    let decoder = ErasureDecoder::new(config);

    // Original data
    let original_data: Vec<u8> = (0..64).collect();

    // Encode into shards
    let shards = encoder.encode(&original_data).unwrap();

    // Only send 3 shards (need at least 4 to decode)
    let shards_to_send: Vec<(usize, &Vec<u8>)> =
        vec![(0, &shards[0]), (1, &shards[1]), (5, &shards[5])];

    let server = WarpEndpoint::server("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = server.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        let conn = server.accept().await.unwrap();
        conn.handshake_server().await.unwrap();

        let mut shard_buffer = ShardBuffer::new(data_shards, parity_shards);

        // Receive only 3 shards
        for _ in 0..3 {
            let frame = conn.recv_frame().await.unwrap();
            if let Frame::Shard {
                shard_idx, data, ..
            } = frame
            {
                shard_buffer.insert(shard_idx, data.to_vec());
            }
        }

        // Should NOT be able to decode
        assert!(!shard_buffer.can_decode());

        // Try to decode anyway - should fail
        let shards_for_decode = shard_buffer.take_shards();
        decoder.decode(&shards_for_decode)
    });

    // Small delay to ensure server is ready
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let client = WarpEndpoint::client().await.unwrap();
    let conn = client.connect(addr, "localhost").await.unwrap();
    conn.handshake().await.unwrap();

    // Send only 3 shards
    for (idx, shard) in shards_to_send {
        conn.send_frame(Frame::Shard {
            chunk_id: 0,
            shard_idx: idx as u16,
            total_shards: 6,
            data: Bytes::from(shard.clone()),
        })
        .await
        .unwrap();
    }

    // Keep connection alive while server processes
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let decode_result = server_task.await.unwrap();
    assert!(
        decode_result.is_err(),
        "Decode should fail with insufficient shards"
    );
}

/// Test multiple chunks with erasure coding
#[tokio::test]
#[ignore = "flaky network test - QUIC connection timing issues"]
async fn test_erasure_e2e_multiple_chunks() {
    let data_shards = 4;
    let parity_shards = 2;
    let config = ErasureConfig::new(data_shards, parity_shards).unwrap();
    let encoder = ErasureEncoder::new(config.clone());
    let decoder = ErasureDecoder::new(config);

    // Create 3 chunks of data
    let chunks: Vec<Vec<u8>> = vec![(0..64).collect(), (64..128).collect(), (128..192).collect()];

    // Encode all chunks
    let encoded_chunks: Vec<Vec<Vec<u8>>> = chunks
        .iter()
        .map(|chunk| encoder.encode(chunk).unwrap())
        .collect();

    let server = WarpEndpoint::server("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let addr = server.local_addr().unwrap();

    let expected_chunks = chunks.clone();
    let server_task = tokio::spawn(async move {
        let conn = server.accept().await.unwrap();
        conn.handshake_server().await.unwrap();

        let mut shard_buffers: HashMap<u32, ShardBuffer> = HashMap::new();

        // Receive all shards for all chunks (3 chunks * 6 shards = 18 frames)
        for _ in 0..18 {
            let frame = conn.recv_frame().await.unwrap();
            if let Frame::Shard {
                chunk_id,
                shard_idx,
                data,
                ..
            } = frame
            {
                let buffer = shard_buffers
                    .entry(chunk_id)
                    .or_insert_with(|| ShardBuffer::new(data_shards, parity_shards));
                buffer.insert(shard_idx, data.to_vec());
            }
        }

        // Decode all chunks and verify
        let mut decoded_chunks = Vec::new();
        for chunk_id in 0..3u32 {
            let buffer = shard_buffers.get_mut(&chunk_id).unwrap();
            assert!(buffer.can_decode());
            let shards = buffer.take_shards();
            let decoded = decoder.decode(&shards).unwrap();
            // Truncate to original chunk size (64 bytes each)
            decoded_chunks.push(decoded[..64].to_vec());
        }
        decoded_chunks
    });

    // Small delay to ensure server is ready
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let client = WarpEndpoint::client().await.unwrap();
    let conn = client.connect(addr, "localhost").await.unwrap();
    conn.handshake().await.unwrap();

    // Send all shards for all chunks
    for (chunk_id, shards) in encoded_chunks.iter().enumerate() {
        for (shard_idx, shard) in shards.iter().enumerate() {
            conn.send_frame(Frame::Shard {
                chunk_id: chunk_id as u32,
                shard_idx: shard_idx as u16,
                total_shards: 6,
                data: Bytes::from(shard.clone()),
            })
            .await
            .unwrap();
        }
    }

    // Keep connection alive while server processes
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let decoded_chunks = server_task.await.unwrap();
    assert_eq!(decoded_chunks, expected_chunks);
}
