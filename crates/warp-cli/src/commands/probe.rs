//! probe command - query remote server capabilities
//!
//! This command connects to a remote warp server and displays its capabilities,
//! including compression support, chunk sizes, GPU availability, and connection latency.

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::time::Instant;
use warp_net::WarpEndpoint;

/// Execute the probe command
///
/// # Arguments
/// * `server` - Server address in format "host:port" (e.g., "localhost:8080")
///
/// # Errors
/// Returns an error if:
/// - The server address is invalid
/// - Connection to the server fails
/// - Handshake with the server fails
pub async fn execute(server: &str) -> Result<()> {
    println!("Probing warp server: {}", server);
    println!();

    let start = Instant::now();

    // Parse server address
    let addr: SocketAddr = server
        .parse()
        .context("Invalid server address. Expected format: host:port (e.g., 127.0.0.1:8080)")?;

    // Create client endpoint
    tracing::debug!("Creating client endpoint");
    let endpoint = WarpEndpoint::client()
        .await
        .context("Failed to create client endpoint")?;

    // Connect to server
    tracing::info!("Connecting to {}", addr);
    print!("Connecting... ");
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let conn = endpoint
        .connect(addr, "warp-server")
        .await
        .context("Failed to connect to server. Is the server running?")?;

    let connect_time = start.elapsed();
    println!("OK ({:.2}ms)", connect_time.as_secs_f64() * 1000.0);

    // Perform handshake to get capabilities
    print!("Handshaking... ");
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let handshake_start = Instant::now();
    let params = conn
        .handshake()
        .await
        .context("Handshake failed. Server may be incompatible.")?;

    let handshake_time = handshake_start.elapsed();
    let total_time = start.elapsed();
    println!("OK ({:.2}ms)", handshake_time.as_secs_f64() * 1000.0);

    println!();

    // Display server capabilities
    println!("Server Capabilities");
    println!("{}", "=".repeat(50));
    println!("  Compression:     {}", params.compression);
    println!("  Hash Algorithm:  {}", params.hash);
    println!("  Chunk Size:      {} KB", params.chunk_size / 1024);
    println!("  Max Streams:     {}", params.parallel_streams);
    println!(
        "  GPU Acceleration: {}",
        if params.use_gpu {
            "Available"
        } else {
            "Not available"
        }
    );
    println!(
        "  Encryption:      {}",
        if params.use_encryption {
            "Enabled"
        } else {
            "Disabled"
        }
    );
    println!(
        "  Deduplication:   {}",
        if params.use_dedup {
            "Enabled"
        } else {
            "Disabled"
        }
    );

    println!();

    // Display connection performance
    println!("Connection Performance");
    println!("{}", "=".repeat(50));
    println!(
        "  Connect Time:    {:.2} ms",
        connect_time.as_secs_f64() * 1000.0
    );
    println!(
        "  Handshake Time:  {:.2} ms",
        handshake_time.as_secs_f64() * 1000.0
    );
    println!(
        "  Total Time:      {:.2} ms",
        total_time.as_secs_f64() * 1000.0
    );
    println!(
        "  Server Address:  {} ({})",
        conn.remote_addr(),
        if is_local_address(&conn.remote_addr()) {
            "local"
        } else {
            "remote"
        }
    );

    println!();

    // Display recommendations
    display_recommendations(&params);

    // Close connection gracefully
    conn.close().await.ok();

    println!();
    println!("Server is reachable and compatible");

    Ok(())
}

/// Display recommendations based on server capabilities
fn display_recommendations(params: &warp_net::NegotiatedParams) {
    println!("Recommendations");
    println!("{}", "=".repeat(50));

    let mut recommendations = Vec::new();

    // Check for GPU
    if !params.use_gpu {
        recommendations.push("Consider enabling GPU acceleration for better performance");
    }

    // Check for encryption
    if !params.use_encryption {
        recommendations.push("Encryption is disabled - data will be sent unencrypted");
    }

    // Check for deduplication
    if params.use_dedup {
        recommendations.push("Deduplication is enabled - duplicate data will be skipped");
    }

    // Check chunk size
    if params.chunk_size < 1024 * 1024 {
        recommendations.push("Small chunk size may reduce throughput on high-bandwidth links");
    } else if params.chunk_size > 16 * 1024 * 1024 {
        recommendations.push("Large chunk size may increase latency on lossy connections");
    }

    // Check parallel streams
    if params.parallel_streams < 4 {
        recommendations.push("Low stream count may limit throughput on high-latency links");
    } else if params.parallel_streams > 32 {
        recommendations.push("High stream count may cause overhead on low-latency links");
    }

    if recommendations.is_empty() {
        println!("  Configuration looks optimal");
    } else {
        for (i, rec) in recommendations.iter().enumerate() {
            println!("  {}. {}", i + 1, rec);
        }
    }
}

/// Check if an address is local (loopback or private)
fn is_local_address(addr: &SocketAddr) -> bool {
    let ip = addr.ip();

    match ip {
        std::net::IpAddr::V4(ipv4) => {
            ipv4.is_loopback() || ipv4.is_private() || ipv4.is_link_local()
        }
        std::net::IpAddr::V6(ipv6) => {
            ipv6.is_loopback() || ipv6.segments()[0] & 0xfe00 == 0xfc00 // ULA
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_address() {
        let addr: Result<SocketAddr, _> = "127.0.0.1:8080".parse();
        assert!(addr.is_ok());

        let addr: Result<SocketAddr, _> = "192.168.1.1:9000".parse();
        assert!(addr.is_ok());

        let addr: Result<SocketAddr, _> = "[::1]:8080".parse();
        assert!(addr.is_ok());
    }

    #[test]
    fn test_parse_invalid_address() {
        let addr: Result<SocketAddr, _> = "invalid".parse();
        assert!(addr.is_err());

        let addr: Result<SocketAddr, _> = "localhost".parse();
        assert!(addr.is_err());

        let addr: Result<SocketAddr, _> = "127.0.0.1".parse();
        assert!(addr.is_err());
    }

    #[test]
    fn test_is_local_address() {
        let loopback: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        assert!(is_local_address(&loopback));

        let private: SocketAddr = "192.168.1.1:8080".parse().unwrap();
        assert!(is_local_address(&private));

        let private2: SocketAddr = "10.0.0.1:8080".parse().unwrap();
        assert!(is_local_address(&private2));

        let public: SocketAddr = "8.8.8.8:8080".parse().unwrap();
        assert!(!is_local_address(&public));

        let ipv6_loopback: SocketAddr = "[::1]:8080".parse().unwrap();
        assert!(is_local_address(&ipv6_loopback));
    }

    #[test]
    fn test_display_recommendations() {
        use warp_net::NegotiatedParams;

        let params = NegotiatedParams {
            compression: "zstd".to_string(),
            hash: "blake3".to_string(),
            chunk_size: 4 * 1024 * 1024,
            parallel_streams: 16,
            use_gpu: true,
            use_encryption: true,
            use_dedup: false,
        };

        display_recommendations(&params);
    }

    #[tokio::test]
    #[ignore = "requires running server"]
    async fn test_probe_local_server() {
        let result = execute("127.0.0.1:8080").await;
        // This will fail without a running server
        assert!(result.is_err());
    }
}
