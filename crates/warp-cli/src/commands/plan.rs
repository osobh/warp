//! plan command implementation - analyzes source and shows transfer plan

use anyhow::{Context, Result};
use console::{style, Term};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use warp_io::walk_directory;

/// Execute the plan command
///
/// Analyzes the source directory or file and displays a detailed transfer plan
/// including file counts, sizes, compression estimates, and transfer time estimates.
pub async fn execute(source: &str, destination: &str) -> Result<()> {
    tracing::info!(
        source = source,
        destination = destination,
        "Planning transfer"
    );

    let source_path = PathBuf::from(source);
    if !source_path.exists() {
        anyhow::bail!("Source path does not exist: {}", source);
    }

    println!("{}", style("Transfer Plan").bold().cyan());
    println!("{}", style("=".repeat(60)).dim());
    println!();
    println!("Source:      {}", source);
    println!("Destination: {}", destination);
    println!();

    // Show analysis spinner
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));
    spinner.set_message("Analyzing source...");

    // Gather statistics
    let stats = if source_path.is_file() {
        analyze_file(&source_path)?
    } else {
        analyze_directory(&source_path)?
    };

    spinner.finish_and_clear();

    // Display results
    display_plan(&stats)?;

    Ok(())
}

/// Statistics about the source
#[derive(Debug)]
struct SourceStats {
    file_count: usize,
    dir_count: usize,
    total_size: u64,
    largest_file: Option<(PathBuf, u64)>,
    extension_stats: HashMap<String, ExtensionStats>,
}

/// Statistics per file extension
#[derive(Debug, Default)]
struct ExtensionStats {
    count: usize,
    total_size: u64,
}

/// Analyze a single file
fn analyze_file(path: &Path) -> Result<SourceStats> {
    let metadata = std::fs::metadata(path)?;
    let size = metadata.len();

    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("(no extension)")
        .to_string();

    let mut extension_stats = HashMap::new();
    extension_stats.insert(
        extension,
        ExtensionStats {
            count: 1,
            total_size: size,
        },
    );

    Ok(SourceStats {
        file_count: 1,
        dir_count: 0,
        total_size: size,
        largest_file: Some((path.to_path_buf(), size)),
        extension_stats,
    })
}

/// Analyze a directory
fn analyze_directory(path: &Path) -> Result<SourceStats> {
    let entries = walk_directory(path).context("Failed to walk directory")?;

    let mut file_count = 0;
    let mut dir_count = 0;
    let mut total_size = 0u64;
    let mut largest_file: Option<(PathBuf, u64)> = None;
    let mut extension_stats: HashMap<String, ExtensionStats> = HashMap::new();

    for entry in entries {
        if entry.is_dir {
            dir_count += 1;
        } else {
            file_count += 1;
            total_size += entry.size;

            // Track largest file
            if let Some((_, current_largest)) = &largest_file {
                if entry.size > *current_largest {
                    largest_file = Some((entry.path.clone(), entry.size));
                }
            } else {
                largest_file = Some((entry.path.clone(), entry.size));
            }

            // Track extension stats
            let extension = entry
                .path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("(no extension)")
                .to_string();

            let ext_stats = extension_stats.entry(extension).or_default();
            ext_stats.count += 1;
            ext_stats.total_size += entry.size;
        }
    }

    Ok(SourceStats {
        file_count,
        dir_count,
        total_size,
        largest_file,
        extension_stats,
    })
}

/// Display the transfer plan
fn display_plan(stats: &SourceStats) -> Result<()> {
    let term = Term::stdout();

    // Basic statistics
    term.write_line(&format!(
        "{}",
        style("Payload Analysis").bold().underlined()
    ))?;
    term.write_line("")?;

    term.write_line(&format!("Files:       {}", style(stats.file_count).cyan()))?;

    if stats.dir_count > 0 {
        term.write_line(&format!(
            "Directories: {}",
            style(stats.dir_count).cyan()
        ))?;
    }

    term.write_line(&format!(
        "Total size:  {}",
        style(format_bytes(stats.total_size)).cyan()
    ))?;

    if let Some((path, size)) = &stats.largest_file {
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        term.write_line(&format!(
            "Largest:     {} ({})",
            style(file_name).cyan(),
            format_bytes(*size)
        ))?;
    }

    term.write_line("")?;

    // File type breakdown (top 5)
    if !stats.extension_stats.is_empty() {
        term.write_line(&format!(
            "{}",
            style("File Types (Top 5 by size)").bold().underlined()
        ))?;
        term.write_line("")?;

        let mut ext_vec: Vec<_> = stats.extension_stats.iter().collect();
        ext_vec.sort_by(|a, b| b.1.total_size.cmp(&a.1.total_size));

        for (ext, ext_stats) in ext_vec.iter().take(5) {
            let percentage = if stats.total_size > 0 {
                (ext_stats.total_size as f64 / stats.total_size as f64) * 100.0
            } else {
                0.0
            };

            term.write_line(&format!(
                "  {:<15} {:>6} files  {:>12}  ({:>5.1}%)",
                style(ext).yellow(),
                ext_stats.count,
                format_bytes(ext_stats.total_size),
                percentage
            ))?;
        }

        term.write_line("")?;
    }

    // Compression estimate
    let estimated_compression_ratio = estimate_compression_ratio(&stats.extension_stats);
    let estimated_compressed_size =
        (stats.total_size as f64 * estimated_compression_ratio) as u64;

    term.write_line(&format!(
        "{}",
        style("Compression Estimate").bold().underlined()
    ))?;
    term.write_line("")?;

    term.write_line(&format!(
        "Original size:    {}",
        style(format_bytes(stats.total_size)).cyan()
    ))?;

    term.write_line(&format!(
        "Estimated size:   {} (zstd)",
        style(format_bytes(estimated_compressed_size)).green()
    ))?;

    term.write_line(&format!(
        "Compression:      {:.1}%",
        (1.0 - estimated_compression_ratio) * 100.0
    ))?;

    term.write_line("")?;

    // Transfer time estimates
    term.write_line(&format!(
        "{}",
        style("Transfer Time Estimates").bold().underlined()
    ))?;
    term.write_line("")?;

    let speeds = [
        ("1 Gbps", 1_000_000_000u64 / 8),
        ("10 Gbps", 10_000_000_000u64 / 8),
        ("25 Gbps", 25_000_000_000u64 / 8),
        ("100 Gbps", 100_000_000_000u64 / 8),
    ];

    for (label, bytes_per_sec) in &speeds {
        let time_secs = estimated_compressed_size as f64 / *bytes_per_sec as f64;
        term.write_line(&format!(
            "  {:<10} {}",
            style(label).yellow(),
            format_duration(time_secs)
        ))?;
    }

    term.write_line("")?;

    // Summary
    term.write_line(&format!("{}", style("Ready to transfer!").bold().green()))?;

    Ok(())
}

/// Estimate compression ratio based on file types
fn estimate_compression_ratio(extension_stats: &HashMap<String, ExtensionStats>) -> f64 {
    if extension_stats.is_empty() {
        return 0.7; // Default estimate
    }

    let mut total_size = 0u64;
    let mut weighted_ratio = 0.0;

    for (ext, stats) in extension_stats {
        total_size += stats.total_size;
        let ratio = get_extension_compression_ratio(ext);
        weighted_ratio += (stats.total_size as f64) * ratio;
    }

    if total_size > 0 {
        weighted_ratio / total_size as f64
    } else {
        0.7
    }
}

/// Get estimated compression ratio for a file extension
fn get_extension_compression_ratio(ext: &str) -> f64 {
    match ext.to_lowercase().as_str() {
        // Already compressed - little to no benefit
        "zip" | "gz" | "bz2" | "xz" | "7z" | "rar" | "tar.gz" | "tgz" => 0.98,
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "mp3" | "mp4" | "avi" | "mov" | "mkv" => 0.95,

        // Binary formats - moderate compression
        "exe" | "dll" | "so" | "dylib" | "bin" | "dat" => 0.75,
        "pdf" | "doc" | "docx" | "ppt" | "pptx" | "xls" | "xlsx" => 0.70,

        // Text formats - good compression
        "txt" | "md" | "log" | "csv" => 0.30,
        "json" | "xml" | "yaml" | "yml" | "toml" | "ini" | "conf" | "config" => 0.35,
        "html" | "htm" | "css" | "js" | "ts" | "jsx" | "tsx" => 0.35,
        "rs" | "c" | "cpp" | "h" | "hpp" | "java" | "py" | "go" | "rb" | "php" => 0.35,
        "sh" | "bash" | "zsh" | "fish" => 0.35,

        // Source code archives
        "tar" => 0.30,

        // Default for unknown
        _ => 0.65,
    }
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

/// Format duration into human-readable string
fn format_duration(seconds: f64) -> String {
    if seconds < 1.0 {
        format!("{:.0} ms", seconds * 1000.0)
    } else if seconds < 60.0 {
        format!("{:.1} seconds", seconds)
    } else if seconds < 3600.0 {
        let minutes = seconds / 60.0;
        format!("{:.1} minutes", minutes)
    } else if seconds < 86400.0 {
        let hours = seconds / 3600.0;
        format!("{:.1} hours", hours)
    } else {
        let days = seconds / 86400.0;
        format!("{:.1} days", days)
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
    fn test_format_duration() {
        assert_eq!(format_duration(0.5), "500 ms");
        assert_eq!(format_duration(1.5), "1.5 seconds");
        assert_eq!(format_duration(90.0), "1.5 minutes");
        assert_eq!(format_duration(7200.0), "2.0 hours");
        assert_eq!(format_duration(172800.0), "2.0 days");
    }

    #[test]
    fn test_compression_ratios() {
        // Text should compress well
        assert!(get_extension_compression_ratio("txt") < 0.5);

        // Images should not compress much
        assert!(get_extension_compression_ratio("jpg") > 0.9);

        // Source code should compress well
        assert!(get_extension_compression_ratio("rs") < 0.5);
    }
}
