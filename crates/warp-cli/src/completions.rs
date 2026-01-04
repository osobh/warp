//! Shell completion generation for warp CLI
//!
//! This module provides functionality to generate shell completion scripts
//! for bash, zsh, fish, powershell, and elvish shells.

use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_complete::{Shell, generate};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Generate shell completions to a writer
///
/// # Arguments
///
/// * `shell` - The shell to generate completions for
/// * `buf` - The writer to output completions to
///
/// # Examples
///
/// ```no_run
/// use clap_complete::Shell;
/// use std::io;
/// # use warp_cli::completions::generate_completions;
/// # use warp_cli::Cli;
///
/// let mut stdout = io::stdout();
/// generate_completions(Shell::Bash, &mut stdout);
/// ```
pub fn generate_completions<W: Write>(shell: Shell, buf: &mut W) -> Result<()> {
    let mut cmd = crate::Cli::command();
    generate(shell, &mut cmd, "warp", buf);
    Ok(())
}

/// Generate shell completion file and save it to a directory
///
/// # Arguments
///
/// * `shell` - The shell to generate completions for
/// * `out_dir` - The directory to save the completion file
///
/// # Returns
///
/// The path to the generated completion file
///
/// # Errors
///
/// Returns an error if:
/// - The output directory does not exist
/// - The file cannot be created or written
pub fn generate_completion_file(shell: Shell, out_dir: &Path) -> Result<PathBuf> {
    if !out_dir.exists() {
        anyhow::bail!("Output directory does not exist: {}", out_dir.display());
    }

    if !out_dir.is_dir() {
        anyhow::bail!("Output path is not a directory: {}", out_dir.display());
    }

    let file_name = get_completion_file_name(shell);
    let file_path = out_dir.join(&file_name);

    let mut file = std::fs::File::create(&file_path)
        .with_context(|| format!("Failed to create completion file: {}", file_path.display()))?;

    generate_completions(shell, &mut file)?;

    Ok(file_path)
}

/// Print shell completions to stdout
///
/// # Arguments
///
/// * `shell` - The shell to generate completions for
pub fn print_completions(shell: Shell) -> Result<()> {
    let mut stdout = std::io::stdout();
    generate_completions(shell, &mut stdout)
}

/// Get the appropriate completion file name for a shell
///
/// # Arguments
///
/// * `shell` - The shell to get the file name for
///
/// # Returns
///
/// The file name for the completion script
fn get_completion_file_name(shell: Shell) -> String {
    match shell {
        Shell::Bash => "warp.bash".to_string(),
        Shell::Zsh => "_warp".to_string(),
        Shell::Fish => "warp.fish".to_string(),
        Shell::PowerShell => "_warp.ps1".to_string(),
        Shell::Elvish => "warp.elv".to_string(),
        _ => format!("warp.{shell}"),
    }
}

/// List all supported shells
pub fn supported_shells() -> &'static [Shell] {
    &[
        Shell::Bash,
        Shell::Zsh,
        Shell::Fish,
        Shell::PowerShell,
        Shell::Elvish,
    ]
}

/// Check if a shell is supported
pub fn is_shell_supported(shell: Shell) -> bool {
    supported_shells().contains(&shell)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use tempfile::TempDir;

    #[test]
    fn test_generate_bash_completions() {
        let mut buf = Cursor::new(Vec::new());
        let result = generate_completions(Shell::Bash, &mut buf);
        assert!(result.is_ok());

        let output = String::from_utf8(buf.into_inner()).unwrap();
        assert!(output.contains("warp"));
        assert!(!output.is_empty());
    }

    #[test]
    fn test_generate_zsh_completions() {
        let mut buf = Cursor::new(Vec::new());
        let result = generate_completions(Shell::Zsh, &mut buf);
        assert!(result.is_ok());

        let output = String::from_utf8(buf.into_inner()).unwrap();
        assert!(output.contains("warp"));
        assert!(!output.is_empty());
    }

    #[test]
    fn test_generate_fish_completions() {
        let mut buf = Cursor::new(Vec::new());
        let result = generate_completions(Shell::Fish, &mut buf);
        assert!(result.is_ok());

        let output = String::from_utf8(buf.into_inner()).unwrap();
        assert!(output.contains("warp"));
        assert!(!output.is_empty());
    }

    #[test]
    fn test_generate_powershell_completions() {
        let mut buf = Cursor::new(Vec::new());
        let result = generate_completions(Shell::PowerShell, &mut buf);
        assert!(result.is_ok());

        let output = String::from_utf8(buf.into_inner()).unwrap();
        assert!(output.contains("warp"));
        assert!(!output.is_empty());
    }

    #[test]
    fn test_generate_elvish_completions() {
        let mut buf = Cursor::new(Vec::new());
        let result = generate_completions(Shell::Elvish, &mut buf);
        assert!(result.is_ok());

        let output = String::from_utf8(buf.into_inner()).unwrap();
        assert!(output.contains("warp"));
        assert!(!output.is_empty());
    }

    #[test]
    fn test_get_completion_file_name_bash() {
        assert_eq!(get_completion_file_name(Shell::Bash), "warp.bash");
    }

    #[test]
    fn test_get_completion_file_name_zsh() {
        assert_eq!(get_completion_file_name(Shell::Zsh), "_warp");
    }

    #[test]
    fn test_get_completion_file_name_fish() {
        assert_eq!(get_completion_file_name(Shell::Fish), "warp.fish");
    }

    #[test]
    fn test_get_completion_file_name_powershell() {
        assert_eq!(get_completion_file_name(Shell::PowerShell), "_warp.ps1");
    }

    #[test]
    fn test_get_completion_file_name_elvish() {
        assert_eq!(get_completion_file_name(Shell::Elvish), "warp.elv");
    }

    #[test]
    fn test_generate_completion_file_success() {
        let temp_dir = TempDir::new().unwrap();
        let result = generate_completion_file(Shell::Bash, temp_dir.path());

        assert!(result.is_ok());
        let file_path = result.unwrap();
        assert!(file_path.exists());
        assert!(file_path.is_file());

        let contents = std::fs::read_to_string(&file_path).unwrap();
        assert!(contents.contains("warp"));
        assert!(!contents.is_empty());
    }

    #[test]
    fn test_generate_completion_file_nonexistent_dir() {
        let non_existent = PathBuf::from("/tmp/warp_test_nonexistent_dir_12345");
        let result = generate_completion_file(Shell::Bash, &non_existent);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn test_generate_completion_file_not_a_dir() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("not_a_dir");
        std::fs::write(&file_path, "test").unwrap();

        let result = generate_completion_file(Shell::Bash, &file_path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not a directory"));
    }

    #[test]
    fn test_supported_shells_contains_all() {
        let shells = supported_shells();
        assert!(shells.contains(&Shell::Bash));
        assert!(shells.contains(&Shell::Zsh));
        assert!(shells.contains(&Shell::Fish));
        assert!(shells.contains(&Shell::PowerShell));
        assert!(shells.contains(&Shell::Elvish));
    }

    #[test]
    fn test_is_shell_supported() {
        assert!(is_shell_supported(Shell::Bash));
        assert!(is_shell_supported(Shell::Zsh));
        assert!(is_shell_supported(Shell::Fish));
        assert!(is_shell_supported(Shell::PowerShell));
        assert!(is_shell_supported(Shell::Elvish));
    }

    #[test]
    fn test_print_completions_bash() {
        // This test verifies print_completions doesn't panic
        // We can't easily capture stdout in tests, but we can verify it runs
        let result = print_completions(Shell::Bash);
        assert!(result.is_ok());
    }

    #[test]
    fn test_completion_contains_subcommands() {
        let mut buf = Cursor::new(Vec::new());
        generate_completions(Shell::Bash, &mut buf).unwrap();
        let output = String::from_utf8(buf.into_inner()).unwrap();

        // Verify that our subcommands appear in the completion
        assert!(output.contains("send") || output.contains("Send"));
        assert!(output.contains("fetch") || output.contains("Fetch"));
    }

    #[test]
    fn test_all_shells_generate_different_output() {
        let mut bash_buf = Cursor::new(Vec::new());
        let mut zsh_buf = Cursor::new(Vec::new());

        generate_completions(Shell::Bash, &mut bash_buf).unwrap();
        generate_completions(Shell::Zsh, &mut zsh_buf).unwrap();

        let bash_output = String::from_utf8(bash_buf.into_inner()).unwrap();
        let zsh_output = String::from_utf8(zsh_buf.into_inner()).unwrap();

        // Different shells should generate different completion scripts
        assert_ne!(bash_output, zsh_output);
    }

    #[test]
    fn test_generate_all_supported_shells() {
        // Verify all supported shells can generate completions without error
        for shell in supported_shells() {
            let mut buf = Cursor::new(Vec::new());
            let result = generate_completions(*shell, &mut buf);
            assert!(
                result.is_ok(),
                "Failed to generate completions for {:?}",
                shell
            );

            let output = String::from_utf8(buf.into_inner()).unwrap();
            assert!(!output.is_empty(), "Empty output for {:?}", shell);
        }
    }
}
