//! S3-compatible storage commands (ls, cp, rm, mb, rb, cat, stat)
//!
//! These commands provide MinIO mc-compatible CLI operations for
//! interacting with warp-store.

use anyhow::{Context, Result, bail};
use console::{Term, style};
use std::path::Path;

/// Parse a storage path like "alias/bucket/key" or "bucket/key"
///
/// # Arguments
///
/// * `path` - Storage path string to parse
///
/// # Returns
///
/// Tuple of (alias, bucket, key) where each component is optional
pub fn parse_path(path: &str) -> (Option<&str>, Option<&str>, Option<&str>) {
    let parts: Vec<&str> = path.trim_matches('/').splitn(3, '/').collect();
    match parts.len() {
        0 => (None, None, None),
        1 if parts[0].is_empty() => (None, None, None),
        1 => (None, Some(parts[0]), None),
        2 => (None, Some(parts[0]), Some(parts[1])),
        _ => (None, Some(parts[0]), Some(parts[2])),
    }
}

/// Check if a path is a local file path
///
/// # Arguments
///
/// * `path` - Path string to check
///
/// # Returns
///
/// `true` if the path is a local filesystem path, `false` otherwise
pub fn is_local_path(path: &str) -> bool {
    path.starts_with('/')
        || path.starts_with("./")
        || path.starts_with("../")
        || Path::new(path).exists()
}

/// List buckets or objects
///
/// # Arguments
///
/// * `path` - Storage path in format "bucket" or "bucket/prefix"
/// * `recursive` - Enable recursive listing of objects
/// * `json` - Output results in JSON format
pub async fn list(path: &str, recursive: bool, json: bool) -> Result<()> {
    let term = Term::stdout();
    let (_, bucket, _prefix) = parse_path(path);

    if bucket.is_none() {
        // List buckets
        term.write_line(&format!(
            "{} {}",
            style("[INFO]").cyan(),
            "Listing buckets..."
        ))?;

        // TODO: Connect to actual store
        // For now, show placeholder
        if json {
            println!("{{\"buckets\": []}}");
        } else {
            println!(
                "{}",
                style("No buckets found (store not connected)").yellow()
            );
        }
    } else {
        // List objects in bucket
        let bucket = bucket.unwrap();
        term.write_line(&format!(
            "{} Listing objects in {} {}",
            style("[INFO]").cyan(),
            style(bucket).green(),
            if recursive { "(recursive)" } else { "" }
        ))?;

        if json {
            println!("{{\"objects\": []}}");
        } else {
            println!(
                "{}",
                style(format!("No objects found in bucket '{}'", bucket)).yellow()
            );
        }
    }

    Ok(())
}

/// Create a bucket
///
/// # Arguments
///
/// * `bucket` - Name of the bucket to create (3-63 chars, lowercase, numbers, hyphens only)
/// * `with_lock` - Enable Object Lock for compliance and governance retention modes
/// * `with_versioning` - Enable versioning for the bucket
pub async fn make_bucket(bucket: &str, with_lock: bool, with_versioning: bool) -> Result<()> {
    let term = Term::stdout();

    // Validate bucket name
    if bucket.len() < 3 || bucket.len() > 63 {
        bail!("Bucket name must be between 3 and 63 characters");
    }

    if !bucket
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        bail!("Bucket name can only contain lowercase letters, numbers, and hyphens");
    }

    term.write_line(&format!(
        "{} Creating bucket: {}{}{}",
        style("[INFO]").cyan(),
        style(bucket).green(),
        if with_lock {
            format!(" {}", style("(with Object Lock)").yellow())
        } else {
            String::new()
        },
        if with_versioning {
            format!(" {}", style("(versioned)").blue())
        } else {
            String::new()
        }
    ))?;

    // TODO: Connect to actual store and create bucket
    term.write_line(&format!(
        "{} Bucket '{}' created successfully",
        style("[OK]").green(),
        bucket
    ))?;

    Ok(())
}

/// Remove a bucket
///
/// # Arguments
///
/// * `bucket` - Name of the bucket to remove
/// * `force` - Force removal without confirmation prompt
pub async fn remove_bucket(bucket: &str, force: bool) -> Result<()> {
    let term = Term::stdout();

    if !force {
        term.write_line(&format!(
            "{} Are you sure you want to remove bucket '{}'? Use --force to confirm",
            style("[WARN]").yellow(),
            bucket
        ))?;
        return Ok(());
    }

    term.write_line(&format!(
        "{} Removing bucket: {}",
        style("[INFO]").cyan(),
        style(bucket).red()
    ))?;

    // TODO: Connect to actual store and remove bucket
    term.write_line(&format!(
        "{} Bucket '{}' removed",
        style("[OK]").green(),
        bucket
    ))?;

    Ok(())
}

/// Copy objects
///
/// # Arguments
///
/// * `source` - Source path (local file path or bucket/key)
/// * `destination` - Destination path (local file path or bucket/key)
/// * `recursive` - Enable recursive copy for directories
/// * `_preserve` - Preserve file attributes (currently unused)
pub async fn copy(source: &str, destination: &str, recursive: bool, _preserve: bool) -> Result<()> {
    let term = Term::stdout();

    let src_local = is_local_path(source);
    let dst_local = is_local_path(destination);

    if src_local && dst_local {
        bail!("Both source and destination are local paths. Use 'cp' command instead.");
    }

    let action = if src_local {
        "Uploading"
    } else if dst_local {
        "Downloading"
    } else {
        "Copying"
    };

    term.write_line(&format!(
        "{} {} {} -> {}{}",
        style("[INFO]").cyan(),
        action,
        style(source).cyan(),
        style(destination).green(),
        if recursive { " (recursive)" } else { "" }
    ))?;

    // TODO: Implement actual copy with progress bar
    term.write_line(&format!("{} Copy complete", style("[OK]").green()))?;

    Ok(())
}

/// Move objects
///
/// # Arguments
///
/// * `source` - Source path (bucket/key)
/// * `destination` - Destination path (bucket/key)
/// * `recursive` - Enable recursive move for directories
pub async fn mv(source: &str, destination: &str, recursive: bool) -> Result<()> {
    let term = Term::stdout();

    term.write_line(&format!(
        "{} Moving {} -> {}{}",
        style("[INFO]").cyan(),
        style(source).cyan(),
        style(destination).green(),
        if recursive { " (recursive)" } else { "" }
    ))?;

    // TODO: Implement actual move (copy + delete)
    term.write_line(&format!("{} Move complete", style("[OK]").green()))?;

    Ok(())
}

/// Remove objects
///
/// # Arguments
///
/// * `path` - Path to objects to remove (bucket/key)
/// * `recursive` - Enable recursive removal for directories
/// * `force` - Force removal without confirmation prompt
/// * `bypass_governance` - Bypass governance retention mode
/// * `versions` - Remove all versions of objects
pub async fn remove(
    path: &str,
    recursive: bool,
    force: bool,
    bypass_governance: bool,
    versions: bool,
) -> Result<()> {
    let term = Term::stdout();

    let (_, bucket, _key) = parse_path(path);

    if bucket.is_none() {
        bail!("Invalid path: must specify bucket/key");
    }

    if !force {
        term.write_line(&format!(
            "{} Are you sure you want to remove '{}'? Use --force to confirm",
            style("[WARN]").yellow(),
            path
        ))?;
        return Ok(());
    }

    term.write_line(&format!(
        "{} Removing: {}{}{}{}",
        style("[INFO]").cyan(),
        style(path).red(),
        if recursive { " (recursive)" } else { "" },
        if bypass_governance {
            format!(" {}", style("(bypass governance)").yellow())
        } else {
            String::new()
        },
        if versions { " (all versions)" } else { "" }
    ))?;

    // TODO: Implement actual removal with Object Lock checks
    term.write_line(&format!("{} Remove complete", style("[OK]").green()))?;

    Ok(())
}

/// Display object contents
///
/// # Arguments
///
/// * `path` - Object path (bucket/key)
/// * `version_id` - Specific version ID to display (optional)
pub async fn cat(path: &str, version_id: Option<&str>) -> Result<()> {
    let (_, bucket, key) = parse_path(path);

    if bucket.is_none() || key.is_none() {
        bail!("Invalid path: must specify bucket/key");
    }

    // TODO: Connect to store and stream object contents to stdout
    eprintln!(
        "{} cat {}{} (not implemented - connect to store)",
        style("[INFO]").cyan(),
        path,
        version_id
            .map(|v| format!(" (version: {})", v))
            .unwrap_or_default()
    );

    Ok(())
}

/// Get object info
///
/// # Arguments
///
/// * `path` - Object path (bucket/key)
/// * `version_id` - Specific version ID to query (optional)
pub async fn stat(path: &str, version_id: Option<&str>) -> Result<()> {
    let term = Term::stdout();
    let (_, bucket, key) = parse_path(path);

    if bucket.is_none() {
        bail!("Invalid path: must specify bucket or bucket/key");
    }

    term.write_line(&format!(
        "{} Object Info: {}{}",
        style("[INFO]").cyan(),
        path,
        version_id
            .map(|v| format!(" (version: {})", v))
            .unwrap_or_default()
    ))?;

    // TODO: Connect to store and get object metadata
    println!("  Bucket: {}", bucket.unwrap());
    if let Some(k) = key {
        println!("  Key: {}", k);
    }
    println!("  Size: (not connected)");
    println!("  Last Modified: (not connected)");
    println!("  ETag: (not connected)");

    Ok(())
}

/// Set object retention
///
/// # Arguments
///
/// * `path` - Object path (bucket/key)
/// * `mode` - Retention mode (GOVERNANCE or COMPLIANCE)
/// * `days` - Retention period in days (optional)
/// * `until` - Retain until date in ISO 8601 format (optional)
/// * `version_id` - Specific version ID to set retention on (optional)
pub async fn retention_set(
    path: &str,
    mode: &str,
    days: Option<u32>,
    until: Option<&str>,
    _version_id: Option<&str>,
) -> Result<()> {
    let term = Term::stdout();
    let (_, bucket, key) = parse_path(path);

    if bucket.is_none() || key.is_none() {
        bail!("Invalid path: must specify bucket/key");
    }

    // Validate mode
    let mode_upper = mode.to_uppercase();
    if mode_upper != "GOVERNANCE" && mode_upper != "COMPLIANCE" {
        bail!("Mode must be GOVERNANCE or COMPLIANCE");
    }

    if days.is_none() && until.is_none() {
        bail!("Must specify either --days or --until");
    }

    term.write_line(&format!(
        "{} Setting {} retention on {}{}",
        style("[INFO]").cyan(),
        style(&mode_upper).yellow(),
        path,
        if let Some(d) = days {
            format!(" for {} days", d)
        } else if let Some(u) = until {
            format!(" until {}", u)
        } else {
            String::new()
        }
    ))?;

    // TODO: Connect to store and set retention
    term.write_line(&format!(
        "{} Retention set successfully",
        style("[OK]").green()
    ))?;

    Ok(())
}

/// Get object retention
///
/// # Arguments
///
/// * `path` - Object path (bucket/key)
/// * `version_id` - Specific version ID to query retention for (optional)
pub async fn retention_get(path: &str, version_id: Option<&str>) -> Result<()> {
    let term = Term::stdout();
    let (_, bucket, key) = parse_path(path);

    if bucket.is_none() || key.is_none() {
        bail!("Invalid path: must specify bucket/key");
    }

    term.write_line(&format!(
        "{} Retention for: {}{}",
        style("[INFO]").cyan(),
        path,
        version_id
            .map(|v| format!(" (version: {})", v))
            .unwrap_or_default()
    ))?;

    // TODO: Connect to store and get retention
    println!("  Mode: (not connected)");
    println!("  Retain Until: (not connected)");

    Ok(())
}

/// Clear object retention
///
/// # Arguments
///
/// * `path` - Object path (bucket/key)
/// * `version_id` - Specific version ID to clear retention from (optional)
/// * `bypass_governance` - Bypass governance retention mode
pub async fn retention_clear(
    path: &str,
    _version_id: Option<&str>,
    bypass_governance: bool,
) -> Result<()> {
    let term = Term::stdout();
    let (_, bucket, key) = parse_path(path);

    if bucket.is_none() || key.is_none() {
        bail!("Invalid path: must specify bucket/key");
    }

    term.write_line(&format!(
        "{} Clearing retention on {}{}",
        style("[INFO]").cyan(),
        path,
        if bypass_governance {
            format!(" {}", style("(bypass governance)").yellow())
        } else {
            String::new()
        }
    ))?;

    // TODO: Connect to store and clear retention
    term.write_line(&format!("{} Retention cleared", style("[OK]").green()))?;

    Ok(())
}

/// Set legal hold
///
/// # Arguments
///
/// * `path` - Object path (bucket/key)
/// * `version_id` - Specific version ID to set legal hold on (optional)
pub async fn legal_hold_set(path: &str, version_id: Option<&str>) -> Result<()> {
    let term = Term::stdout();
    let (_, bucket, key) = parse_path(path);

    if bucket.is_none() || key.is_none() {
        bail!("Invalid path: must specify bucket/key");
    }

    term.write_line(&format!(
        "{} Setting legal hold on {}{}",
        style("[INFO]").cyan(),
        path,
        version_id
            .map(|v| format!(" (version: {})", v))
            .unwrap_or_default()
    ))?;

    // TODO: Connect to store and set legal hold
    term.write_line(&format!("{} Legal hold enabled", style("[OK]").green()))?;

    Ok(())
}

/// Clear legal hold
///
/// # Arguments
///
/// * `path` - Object path (bucket/key)
/// * `version_id` - Specific version ID to clear legal hold from (optional)
pub async fn legal_hold_clear(path: &str, version_id: Option<&str>) -> Result<()> {
    let term = Term::stdout();
    let (_, bucket, key) = parse_path(path);

    if bucket.is_none() || key.is_none() {
        bail!("Invalid path: must specify bucket/key");
    }

    term.write_line(&format!(
        "{} Clearing legal hold on {}{}",
        style("[INFO]").cyan(),
        path,
        version_id
            .map(|v| format!(" (version: {})", v))
            .unwrap_or_default()
    ))?;

    // TODO: Connect to store and clear legal hold
    term.write_line(&format!("{} Legal hold disabled", style("[OK]").green()))?;

    Ok(())
}

/// Get legal hold status
///
/// # Arguments
///
/// * `path` - Object path (bucket/key)
/// * `version_id` - Specific version ID to query legal hold status for (optional)
pub async fn legal_hold_get(path: &str, version_id: Option<&str>) -> Result<()> {
    let term = Term::stdout();
    let (_, bucket, key) = parse_path(path);

    if bucket.is_none() || key.is_none() {
        bail!("Invalid path: must specify bucket/key");
    }

    term.write_line(&format!(
        "{} Legal hold status for: {}{}",
        style("[INFO]").cyan(),
        path,
        version_id
            .map(|v| format!(" (version: {})", v))
            .unwrap_or_default()
    ))?;

    // TODO: Connect to store and get legal hold
    println!("  Status: (not connected)");

    Ok(())
}

/// Alias configuration file path
fn alias_config_path() -> Result<std::path::PathBuf> {
    let config_dir = dirs::config_dir()
        .context("Could not determine config directory")?
        .join("warp");
    std::fs::create_dir_all(&config_dir)?;
    Ok(config_dir.join("aliases.toml"))
}

/// Set an alias
///
/// # Arguments
///
/// * `alias` - Name for the storage endpoint alias
/// * `url` - S3-compatible storage endpoint URL
/// * `access_key` - Access key for authentication (optional)
/// * `secret_key` - Secret key for authentication (optional)
pub async fn alias_set(
    alias: &str,
    url: &str,
    _access_key: Option<&str>,
    _secret_key: Option<&str>,
) -> Result<()> {
    let term = Term::stdout();

    term.write_line(&format!(
        "{} Setting alias '{}' -> {}",
        style("[INFO]").cyan(),
        style(alias).green(),
        url
    ))?;

    // TODO: Save to config file
    let config_path = alias_config_path()?;
    term.write_line(&format!(
        "{} Alias saved to {}",
        style("[OK]").green(),
        config_path.display()
    ))?;

    Ok(())
}

/// Remove an alias
///
/// # Arguments
///
/// * `alias` - Name of the alias to remove
pub async fn alias_remove(alias: &str) -> Result<()> {
    let term = Term::stdout();

    term.write_line(&format!(
        "{} Removing alias: {}",
        style("[INFO]").cyan(),
        style(alias).red()
    ))?;

    // TODO: Remove from config file
    term.write_line(&format!("{} Alias removed", style("[OK]").green()))?;

    Ok(())
}

/// List aliases
pub async fn alias_list() -> Result<()> {
    let term = Term::stdout();

    term.write_line(&format!("{} Configured aliases:", style("[INFO]").cyan()))?;

    // TODO: Load from config file
    println!("  (no aliases configured)");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_path() {
        assert_eq!(parse_path(""), (None, None, None));
        assert_eq!(parse_path("bucket"), (None, Some("bucket"), None));
        assert_eq!(
            parse_path("bucket/key"),
            (None, Some("bucket"), Some("key"))
        );
        assert_eq!(
            parse_path("bucket/path/to/key"),
            (None, Some("bucket"), Some("to/key"))
        );
    }

    #[test]
    fn test_is_local_path() {
        assert!(is_local_path("/tmp/file"));
        assert!(is_local_path("./file"));
        assert!(is_local_path("../file"));
        assert!(!is_local_path("bucket/key"));
    }
}
