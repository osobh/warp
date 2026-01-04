//! Command handler for generating shell completions

use anyhow::Result;
use clap_complete::Shell;
use std::path::Path;

/// Execute the completions command
///
/// # Arguments
///
/// * `shell` - The shell to generate completions for
/// * `output` - Optional output directory path. If None, prints to stdout
///
/// # Returns
///
/// Returns Ok(()) on success, or an error if generation fails
///
/// # Examples
///
/// ```no_run
/// use clap_complete::Shell;
/// # use warp_cli::commands::completions::execute;
///
/// // Print to stdout
/// execute(Shell::Bash, None).unwrap();
///
/// // Save to file
/// use std::path::Path;
/// execute(Shell::Bash, Some(Path::new("/tmp"))).unwrap();
/// ```
pub async fn execute(shell: Shell, output: Option<&Path>) -> Result<()> {
    match output {
        Some(dir) => {
            let file_path = crate::completions::generate_completion_file(shell, dir)?;
            println!("Generated {} completion: {}", shell, file_path.display());
        }
        None => {
            crate::completions::print_completions(shell)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_execute_to_stdout() {
        // This test verifies execute doesn't panic when printing to stdout
        let result = execute(Shell::Bash, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_to_file_bash() {
        let temp_dir = TempDir::new().unwrap();
        let result = execute(Shell::Bash, Some(temp_dir.path())).await;

        assert!(result.is_ok());

        let expected_file = temp_dir.path().join("warp.bash");
        assert!(expected_file.exists());

        let contents = std::fs::read_to_string(&expected_file).unwrap();
        assert!(contents.contains("warp"));
    }

    #[tokio::test]
    async fn test_execute_to_file_zsh() {
        let temp_dir = TempDir::new().unwrap();
        let result = execute(Shell::Zsh, Some(temp_dir.path())).await;

        assert!(result.is_ok());

        let expected_file = temp_dir.path().join("_warp");
        assert!(expected_file.exists());
    }

    #[tokio::test]
    async fn test_execute_to_file_fish() {
        let temp_dir = TempDir::new().unwrap();
        let result = execute(Shell::Fish, Some(temp_dir.path())).await;

        assert!(result.is_ok());

        let expected_file = temp_dir.path().join("warp.fish");
        assert!(expected_file.exists());
    }

    #[tokio::test]
    async fn test_execute_to_nonexistent_directory() {
        let non_existent = std::path::PathBuf::from("/tmp/warp_nonexistent_test_99999");
        let result = execute(Shell::Bash, Some(&non_existent)).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_all_shells_to_directory() {
        let temp_dir = TempDir::new().unwrap();

        for shell in crate::completions::supported_shells() {
            let result = execute(*shell, Some(temp_dir.path())).await;
            assert!(result.is_ok(), "Failed to execute for shell: {:?}", shell);
        }

        // Verify files were created
        assert!(temp_dir.path().join("warp.bash").exists());
        assert!(temp_dir.path().join("_warp").exists());
        assert!(temp_dir.path().join("warp.fish").exists());
        assert!(temp_dir.path().join("_warp.ps1").exists());
        assert!(temp_dir.path().join("warp.elv").exists());
    }
}
