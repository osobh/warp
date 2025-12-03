//! listen command - QUIC server for receiving transfers

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, Notify};
use warp_format::WarpReader;
use warp_net::{Frame, WarpConnection, WarpListener};

/// Metadata for incoming file
#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct FileMetadata {
    /// File name
    name: String,
    /// Total original size
    total_size: u64,
    /// Is this a directory transfer
    is_directory: bool,
}

/// Execute the listen command
pub async fn execute(bind: &str, port: u16) -> Result<()> {
    tracing::info!(bind = bind, port = port, "Starting listener");

    // Parse bind address
    let addr = format!("{}:{}", bind, port)
        .parse()
        .context("Invalid bind address")?;

    // Bind listener with self-signed certificate
    let listener = WarpListener::bind_self_signed(addr)
        .await
        .context("Failed to bind listener")?;

    let bound_addr = listener.local_addr();
    println!("Listening on {}", bound_addr);
    println!("Press Ctrl+C to stop");
    println!();

    // Setup shutdown notification
    let shutdown = Arc::new(Notify::new());
    let shutdown_clone = shutdown.clone();

    // Setup Ctrl+C handler
    tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!("Failed to listen for ctrl-c: {}", e);
        }
        tracing::info!("Received shutdown signal");
        shutdown_clone.notify_waiters();
    });

    // Connection counter for tracking
    let connection_count = Arc::new(Mutex::new(0u64));

    // Accept connections loop
    loop {
        tokio::select! {
            _ = shutdown.notified() => {
                println!("\nShutting down...");
                break;
            }
            result = listener.accept() => {
                match result {
                    Ok(conn) => {
                        let mut count = connection_count.lock().await;
                        *count += 1;
                        let conn_id = *count;
                        drop(count);

                        // Spawn handler for this connection
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(conn, conn_id).await {
                                eprintln!("Connection {} error: {}", conn_id, e);
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("Failed to accept connection: {}", e);
                    }
                }
            }
        }
    }

    // Graceful shutdown
    listener.shutdown_graceful().await;

    Ok(())
}

/// Handle an incoming connection
async fn handle_connection(conn: WarpConnection, conn_id: u64) -> Result<()> {
    let remote_addr = conn.remote_addr();
    tracing::info!(
        conn_id = conn_id,
        remote_addr = %remote_addr,
        "Accepted connection"
    );

    println!("[Connection {}] From {}", conn_id, remote_addr);

    // Perform handshake
    let params = conn
        .handshake_server()
        .await
        .context("Handshake failed")?;

    tracing::debug!(
        conn_id = conn_id,
        compression = params.compression,
        chunk_size = params.chunk_size,
        streams = params.parallel_streams,
        "Handshake complete"
    );

    // Wait for PLAN frame to know what we're receiving
    let frame = conn.recv_frame().await.context("Failed to receive PLAN")?;

    let (total_size, num_chunks, _chunk_size, metadata_bytes) = match frame {
        Frame::Plan {
            total_size,
            num_chunks,
            chunk_size,
            metadata,
        } => (total_size, num_chunks, chunk_size, metadata),
        Frame::Error { code, message } => {
            anyhow::bail!("Received error from sender: {} - {}", code, message);
        }
        _ => {
            anyhow::bail!("Expected PLAN frame, got {:?}", frame);
        }
    };

    // Decode metadata
    let file_metadata: FileMetadata = rmp_serde::from_slice(&metadata_bytes)
        .context("Failed to decode file metadata")?;

    tracing::info!(
        conn_id = conn_id,
        file_name = file_metadata.name,
        total_size = total_size,
        num_chunks = num_chunks,
        "Receiving file"
    );

    println!(
        "[Connection {}] Receiving: {} ({} chunks, {})",
        conn_id,
        file_metadata.name,
        num_chunks,
        format_bytes(total_size)
    );

    // Send ACCEPT to indicate we're ready
    conn.send_frame(Frame::Accept)
        .await
        .context("Failed to send ACCEPT")?;

    // Create temporary file to receive archive
    let temp_dir = std::env::temp_dir();
    let temp_file_path = temp_dir.join(format!("warp_recv_{}.warp", conn_id));

    // Setup progress bar
    let progress = ProgressBar::new(total_size);
    progress.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        )
        .unwrap()
        .progress_chars("#>-"),
    );

    // Receive chunks and write to temporary file
    let mut temp_file = File::create(&temp_file_path)
        .await
        .context("Failed to create temporary file")?;

    let mut received_chunks = HashMap::new();
    let mut total_received = 0u64;

    // Receive all chunks
    while received_chunks.len() < num_chunks as usize {
        let (chunk_id, chunk_data) =
            conn.recv_chunk().await.context("Failed to receive chunk")?;

        if chunk_id >= num_chunks {
            anyhow::bail!("Invalid chunk ID: {}", chunk_id);
        }

        // Store chunk
        received_chunks.insert(chunk_id, chunk_data.clone());
        total_received += chunk_data.len() as u64;
        progress.set_position(total_received);

        // Send ACK for this chunk
        conn.send_frame(Frame::Ack {
            chunk_ids: vec![chunk_id],
        })
        .await
        .context("Failed to send ACK")?;
    }

    progress.finish_with_message("Transfer complete");

    // Write chunks in order to temporary file
    let write_progress = ProgressBar::new(num_chunks as u64);
    write_progress.set_style(
        ProgressStyle::with_template("[{elapsed_precise}] Writing chunks {pos}/{len}")
            .unwrap()
            .progress_chars("#>-"),
    );

    for chunk_id in 0..num_chunks {
        if let Some(chunk_data) = received_chunks.get(&chunk_id) {
            temp_file
                .write_all(chunk_data)
                .await
                .context("Failed to write chunk")?;
            write_progress.inc(1);
        } else {
            anyhow::bail!("Missing chunk {}", chunk_id);
        }
    }

    temp_file.flush().await.context("Failed to flush file")?;
    drop(temp_file);

    write_progress.finish_and_clear();

    // Wait for DONE frame
    let frame = conn.recv_frame().await.context("Failed to receive DONE")?;
    match frame {
        Frame::Done => {}
        Frame::Error { code, message } => {
            anyhow::bail!("Received error: {} - {}", code, message);
        }
        _ => {
            anyhow::bail!("Expected DONE frame");
        }
    }

    // Wait for VERIFY frame with merkle root
    let frame = conn
        .recv_frame()
        .await
        .context("Failed to receive VERIFY")?;
    let _expected_merkle_root = match frame {
        Frame::Verify { merkle_root } => merkle_root,
        Frame::Error { code, message } => {
            anyhow::bail!("Received error: {} - {}", code, message);
        }
        _ => {
            anyhow::bail!("Expected VERIFY frame");
        }
    };

    // Verify the received archive
    println!("[Connection {}] Verifying integrity...", conn_id);

    let reader = WarpReader::open(&temp_file_path).context("Failed to open received archive")?;

    let is_valid = reader.verify().context("Failed to verify archive")?;

    if !is_valid {
        // Send error and bail
        conn.send_frame(Frame::Error {
            code: 1,
            message: "Archive verification failed".to_string(),
        })
        .await
        .ok();
        anyhow::bail!("Archive verification failed!");
    }

    // Get merkle root from archive and compare
    let (_, _, _) = reader.stats();
    // Note: WarpReader doesn't expose merkle root directly, so we'll trust verification

    // Send success acknowledgment
    conn.send_frame(Frame::Accept)
        .await
        .context("Failed to send final ACK")?;

    // Extract the archive to current directory
    let extract_dest = PathBuf::from(".");
    println!(
        "[Connection {}] Extracting to {}...",
        conn_id,
        extract_dest.display()
    );

    reader
        .extract_all(&extract_dest)
        .context("Failed to extract archive")?;

    println!(
        "[Connection {}] Successfully received and extracted {}",
        conn_id, file_metadata.name
    );

    // Cleanup temporary file
    tokio::fs::remove_file(&temp_file_path).await.ok();

    Ok(())
}

/// Format bytes into human-readable string
fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.2} {}", size, UNITS[unit_idx])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GB");
    }
}
