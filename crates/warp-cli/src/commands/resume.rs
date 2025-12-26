//! resume command - resume interrupted transfers
//!
//! This command allows resuming previously interrupted or paused transfer sessions.
//! Sessions are loaded from disk and the transfer continues from where it left off.

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::sync::Arc;
use warp_core::{Session, SessionState, TransferConfig, TransferEngine, TransferProgress};

/// Execute the resume command
///
/// # Arguments
/// * `session_id` - Session ID to resume, or "list" to show all available sessions
///
/// # Errors
/// Returns an error if:
/// - The session cannot be loaded
/// - The session cannot be resumed (invalid state or no progress)
/// - The transfer fails
pub async fn execute(session_id: &str) -> Result<()> {
    let sessions_dir = Session::sessions_dir();

    // Special case: list all available sessions
    if session_id == "list" {
        return list_sessions(&sessions_dir).await;
    }

    // Load the session
    let mut session = Session::load(&sessions_dir, session_id)
        .with_context(|| format!("Failed to load session: {}", session_id))?;

    // Check if session can be resumed
    if !session.can_resume() {
        anyhow::bail!(
            "Session {} cannot be resumed (state: {:?}, progress: {:.1}%)",
            session_id,
            session.state,
            session.progress()
        );
    }

    // Display session information
    println!("Resuming Transfer Session");
    println!("{}", "=".repeat(60));
    println!("Session ID:  {}", &session.id[..12]);
    println!("Source:      {}", session.source.display());
    println!("Destination: {}", session.destination);
    println!("Progress:    {:.1}%", session.progress());
    println!(
        "Status:      {:?} ({} of {} chunks completed)",
        session.state,
        session.completed_chunks.len(),
        session.total_chunks
    );

    if let Some(ref error) = session.error_message {
        println!("Last Error:  {}", error);
    }

    // Display erasure coding state if present
    if let Some(ref erasure_state) = session.erasure_state {
        println!();
        println!("Erasure Coding: {}:{} (data:parity shards)",
            erasure_state.data_shards, erasure_state.parity_shards);
        println!("Decoded chunks: {}", erasure_state.decoded_chunks.len());

        let partial = erasure_state.partial_chunks();
        if !partial.is_empty() {
            println!("Partial chunks: {} (need more shards)", partial.len());
            // Show details for up to 5 partial chunks
            for chunk_id in partial.iter().take(5) {
                let needed = erasure_state.shards_needed(*chunk_id);
                let received = erasure_state.received_shards.get(chunk_id)
                    .map(|v| v.len())
                    .unwrap_or(0);
                println!("  Chunk {}: {}/{} shards ({} more needed)",
                    chunk_id, received, erasure_state.data_shards, needed);
            }
            if partial.len() > 5 {
                println!("  ... and {} more partial chunks", partial.len() - 5);
            }
        }
    }

    println!();

    // Create progress bar
    let pb = ProgressBar::new(session.total_chunks);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} chunks ({percent}%) | {msg}",
        )
        .unwrap()
        .progress_chars("#>-"),
    );

    pb.set_position(session.completed_chunks.len() as u64);

    // Create transfer engine with progress callback
    let pb_clone = pb.clone();
    let progress_callback = Arc::new(move |progress: TransferProgress| {
        pb_clone.set_position(progress.chunks_completed);
        pb_clone.set_message(format!(
            "{:.2} MB/s",
            progress.bytes_per_second / 1_048_576.0
        ));
    });

    let config = TransferConfig::default();
    let engine = TransferEngine::new(config).with_progress(progress_callback);

    // Save session before starting
    session
        .save(&sessions_dir)
        .context("Failed to save session state")?;

    // Resume the transfer
    tracing::info!("Resuming transfer for session {}", session.id);
    let result = engine.resume(&mut session).await;

    // Update progress bar
    pb.finish_and_clear();

    match result {
        Ok(()) => {
            println!("Transfer Completed Successfully");
            println!("{}", "=".repeat(60));
            println!("Session:     {}", &session.id[..12]);
            println!("Chunks:      {}", session.total_chunks);
            println!("Transferred: {:.2} MB", session.transferred_bytes as f64 / 1_048_576.0);

            // Clean up completed session
            if let Err(e) = Session::delete(&sessions_dir, &session.id) {
                tracing::warn!("Failed to delete completed session: {}", e);
            }

            Ok(())
        }
        Err(e) => {
            // Save failed session state for future retry
            session.set_state(SessionState::Failed);
            if let Err(save_err) = session.save(&sessions_dir) {
                tracing::error!("Failed to save session after error: {}", save_err);
            }

            anyhow::bail!("Transfer failed: {}", e)
        }
    }
}

/// List all available sessions
async fn list_sessions(sessions_dir: &PathBuf) -> Result<()> {
    let sessions = Session::list(sessions_dir).context("Failed to list sessions")?;

    if sessions.is_empty() {
        println!("No saved sessions found.");
        println!();
        println!("Sessions are saved when transfers are interrupted or paused.");
        println!("Session directory: {}", sessions_dir.display());
        return Ok(());
    }

    println!("Available Sessions");
    println!("{}", "=".repeat(95));
    println!(
        "{:<14} {:<23} {:<10} {:<15} {:<10} {:<8}",
        "Session ID", "Source", "Progress", "State", "Chunks", "Erasure"
    );
    println!("{}", "-".repeat(95));

    for session in sessions {
        let source_name = session
            .source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        // Truncate source name if too long
        let source_display = if source_name.len() > 22 {
            format!("{}...", &source_name[..19])
        } else {
            source_name.to_string()
        };

        // For erasure-coded sessions, use decoded_chunks count
        let completed = if let Some(ref es) = session.erasure_state {
            es.decoded_chunks.len()
        } else {
            session.completed_chunks.len()
        };

        // Show erasure coding status
        let erasure_status = if let Some(ref es) = session.erasure_state {
            format!("{}:{}", es.data_shards, es.parity_shards)
        } else {
            "-".to_string()
        };

        println!(
            "{:<14} {:<23} {:>9.1}% {:<15} {:>4}/{:<4} {:<8}",
            &session.id[..12],
            source_display,
            session.progress(),
            format!("{:?}", session.state),
            completed,
            session.total_chunks,
            erasure_status
        );
    }

    println!();
    println!("To resume a session: warp resume <session_id>");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_list_empty_sessions() {
        let dir = tempdir().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            let result = list_sessions(&dir.path().to_path_buf()).await;
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_list_with_sessions() {
        let dir = tempdir().unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            // Create a test session
            let mut session = Session::new(
                PathBuf::from("/tmp/test.txt"),
                "dest".to_string(),
            );
            session.total_chunks = 10;
            session.complete_chunk(5);
            session.set_state(SessionState::Paused);
            session.save(dir.path()).unwrap();

            let result = list_sessions(&dir.path().to_path_buf()).await;
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_resume_nonexistent_session() {
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            let result = execute("nonexistent").await;
            assert!(result.is_err());
        });
    }
}
