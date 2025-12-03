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
    println!("{}", "=".repeat(90));
    println!(
        "{:<14} {:<25} {:<10} {:<15} {:<10}",
        "Session ID", "Source", "Progress", "State", "Chunks"
    );
    println!("{}", "-".repeat(90));

    for session in sessions {
        let source_name = session
            .source
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        // Truncate source name if too long
        let source_display = if source_name.len() > 24 {
            format!("{}...", &source_name[..21])
        } else {
            source_name.to_string()
        };

        println!(
            "{:<14} {:<25} {:>9.1}% {:<15} {:>4}/{:<4}",
            &session.id[..12],
            source_display,
            session.progress(),
            format!("{:?}", session.state),
            session.completed_chunks.len(),
            session.total_chunks
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
