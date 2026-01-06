//! fetch command implementation - extracts .warp archives or fetches from remote

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Instant;
use warp_crypto::kdf::derive_key;
use warp_format::{EncryptionKey, WarpReader};

/// Get password from argument or prompt user for decryption
fn get_decryption_password(password: Option<&str>) -> Result<String> {
    if let Some(p) = password {
        Ok(p.to_string())
    } else {
        // Prompt for password
        print!("Enter decryption password: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let password = input.trim().to_string();

        if password.is_empty() {
            anyhow::bail!("Password cannot be empty");
        }

        Ok(password)
    }
}

/// Execute the fetch command
///
/// Extracts a .warp archive to the destination path. If the source ends with .warp
/// and exists locally, extracts the local archive. Otherwise, fetches from remote
/// server (if source contains host:port format).
pub async fn execute(source: &str, destination: &str, password: Option<&str>) -> Result<()> {
    tracing::info!(source = source, destination = destination, "Starting fetch");

    // Determine if this is a local archive extraction or remote fetch
    if is_remote_source(source) {
        // Remote fetch
        let (host, port, remote_path) = parse_remote_source(source)?;
        fetch_remote(&host, port, &remote_path, destination).await
    } else {
        // Local archive extraction
        extract_local_archive(source, destination, password).await
    }
}

/// Check if source is remote (contains host:port)
fn is_remote_source(source: &str) -> bool {
    // Remote format: "host:port" or "host:port/path"
    // Local format: "/path/to/file.warp" or "file.warp"
    if source.starts_with('/') || source.starts_with('.') {
        return false;
    }
    if source.ends_with(".warp") && !source.contains(':') {
        return false;
    }
    source.contains(':')
}

/// Parse remote source into (host, port, path)
fn parse_remote_source(source: &str) -> Result<(String, u16, String)> {
    // Format: "host:port/path" or "host:port"
    // Example: "192.168.1.1:9999/backup" -> ("192.168.1.1", 9999, "/backup")

    let parts: Vec<&str> = source.splitn(2, '/').collect();
    let host_port = parts[0];
    let remote_path = if parts.len() > 1 {
        format!("/{}", parts[1])
    } else {
        String::from("")
    };

    // Parse host:port
    let host_port_parts: Vec<&str> = host_port.splitn(2, ':').collect();
    if host_port_parts.len() != 2 {
        anyhow::bail!("Invalid remote source format. Expected 'host:port' or 'host:port/path'");
    }

    let host = host_port_parts[0].to_string();
    let port: u16 = host_port_parts[1].parse().context("Invalid port number")?;

    Ok((host, port, remote_path))
}

/// Fetch from remote server
///
/// Note: This implementation assumes the server is running `warp listen` which
/// receives files. For true bidirectional fetch, we would need a request/response
/// protocol. For now, this is a placeholder that shows the intended structure.
async fn fetch_remote(
    _host: &str,
    _port: u16,
    _remote_path: &str,
    _destination: &str,
) -> Result<()> {
    // TODO: Implement actual remote fetch protocol
    // This would require:
    // 1. Connect to server
    // 2. Send a REQUEST frame with the file path to fetch
    // 3. Receive PLAN frame with file details
    // 4. Receive chunks
    // 5. Assemble and extract
    //
    // For now, remote fetch is not fully implemented because the current
    // protocol is sender-initiated (send pushes to listen).
    // A full implementation would need a REQUEST frame type and
    // server-side file serving logic.

    anyhow::bail!(
        "Remote fetch is not yet implemented. The current protocol supports:\n\
         - Local archive creation: warp send <source> <dest.warp>\n\
         - Local extraction: warp fetch <source.warp> <dest>\n\
         - Remote send: warp send <source> <host:port>\n\
         \n\
         To fetch from remote, run 'warp send' on the remote machine\n\
         and 'warp listen' on this machine."
    );
}

/// Extract a local .warp archive
async fn extract_local_archive(
    source: &str,
    destination: &str,
    password: Option<&str>,
) -> Result<()> {
    let start_time = Instant::now();

    // Parse source path
    let source_path = PathBuf::from(source);
    if !source_path.exists() {
        anyhow::bail!("Source archive does not exist: {}", source);
    }

    if !source_path.is_file() {
        anyhow::bail!("Source is not a file: {}", source);
    }

    // Parse destination path
    let dest_path = PathBuf::from(destination);

    // Create destination directory if it doesn't exist
    if !dest_path.exists() {
        std::fs::create_dir_all(&dest_path).context("Failed to create destination directory")?;
    }

    println!("Extracting archive: {}", source_path.display());
    println!("Destination: {}", dest_path.display());
    println!();

    // Show opening spinner
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));
    spinner.set_message("Opening archive...");

    // First, try to open without key to check if encrypted
    let temp_reader = WarpReader::open(&source_path).ok();
    let is_encrypted = temp_reader.as_ref().is_some_and(|r| r.is_encrypted());
    drop(temp_reader);

    // Open the archive (with or without decryption key)
    let reader = if is_encrypted {
        println!("Archive is encrypted");
        let password = get_decryption_password(password)?;

        // We need to read the salt from the header
        // For now, we'll use a simple approach - derive key and try to open
        // In a proper implementation, we'd read the salt from header first
        let reader_for_salt =
            WarpReader::open(&source_path).context("Failed to open archive for salt")?;
        let salt = reader_for_salt.header().salt;
        drop(reader_for_salt);

        let derived =
            derive_key(password.as_bytes(), &salt).context("Failed to derive decryption key")?;
        let key = EncryptionKey::from_bytes(*derived.as_bytes());

        WarpReader::open_encrypted(&source_path, key)
            .context("Failed to open encrypted archive. Wrong password?")?
    } else {
        WarpReader::open(&source_path).context("Failed to open archive")?
    };

    // Get archive info
    let file_count = reader.file_count();
    let (original_size, compressed_size, ratio) = reader.stats();

    spinner.set_message(format!("Extracting {} files...", file_count));

    // Extract all files
    reader
        .extract_all(&dest_path)
        .context("Failed to extract archive")?;

    spinner.set_message("Verifying integrity...");

    // Verify the archive integrity
    let is_valid = reader.verify().context("Failed to verify archive")?;

    spinner.finish_and_clear();

    if !is_valid {
        anyhow::bail!("Archive verification failed! Data may be corrupted.");
    }

    let duration = start_time.elapsed();

    // Print summary
    println!("Archive extracted successfully");
    println!();
    println!("Files extracted: {}", file_count);
    println!("Total size: {}", format_bytes(original_size));
    println!("Compressed size: {}", format_bytes(compressed_size));
    println!("Compression ratio: {:.1}%", (1.0 - ratio) * 100.0);
    println!("Duration: {:.2}s", duration.as_secs_f64());

    if original_size > 0 {
        let throughput = original_size as f64 / duration.as_secs_f64();
        println!("Throughput: {}/s", format_bytes(throughput as u64));
    }

    println!();
    println!("Integrity verification passed");

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

    #[test]
    fn test_is_remote_source() {
        assert!(is_remote_source("192.168.1.1:9999"));
        assert!(is_remote_source("192.168.1.1:9999/backup"));
        assert!(is_remote_source("localhost:8080"));
        assert!(!is_remote_source("/tmp/test.warp"));
        assert!(!is_remote_source("./test.warp"));
        assert!(!is_remote_source("test.warp"));
        assert!(!is_remote_source("/var/data.warp"));
    }

    #[test]
    fn test_parse_remote_source() {
        let (host, port, path) = parse_remote_source("192.168.1.1:9999").unwrap();
        assert_eq!(host, "192.168.1.1");
        assert_eq!(port, 9999);
        assert_eq!(path, "");

        let (host, port, path) = parse_remote_source("192.168.1.1:9999/backup").unwrap();
        assert_eq!(host, "192.168.1.1");
        assert_eq!(port, 9999);
        assert_eq!(path, "/backup");

        let (host, port, path) = parse_remote_source("localhost:8080/test/path").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 8080);
        assert_eq!(path, "/test/path");

        assert!(parse_remote_source("invalid").is_err());
        assert!(parse_remote_source("localhost").is_err());
    }
}
