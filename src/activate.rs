//! Crate activation: resolve a registry path to cached manifests, create an
//! ephemeral shimlink directory, and exec a subshell with the shimlink
//! dir prepended to PATH. Auto-fetches manifests from the registry if not cached.

use anyhow::{Context, Result, bail};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use crate::config::BulkerConfig;
use crate::imports;
use crate::manifest::CrateVars;
use crate::shimlink;

/// Result of building a new PATH: the full PATH string and the shimdir path.
pub struct ActivationResult {
    /// The full PATH string (shimdir only in strict mode, shimdir:existing in normal mode).
    pub path: String,
    /// The shimlink directory path (for cleanup on deactivation).
    pub shimdir: String,
}

/// Build the new PATH using shimlink directories.
/// Creates a temp directory with symlinks to the bulker binary for each command,
/// then returns the PATH string with the shimlink dir prepended.
/// Auto-fetches manifests from the registry if not cached locally.
pub fn get_new_path(config: &BulkerConfig, cratelist: &[CrateVars], strict: bool, force: bool) -> Result<ActivationResult> {
    // Each activation gets its own shimdir. Sharing a shimdir between shells
    // is a correctness bug: re-activation nukes a live shell's PATH.
    let shimdir = tempfile::Builder::new()
        .prefix("bulker_")
        .tempdir()
        .context("Failed to create shimlink temp directory")?
        .keep();

    // Auto-fetch: ensure all manifests (and their imports) are cached
    for cv in cratelist {
        let mut visited = std::collections::HashSet::new();
        crate::manifest_cache::ensure_cached_with_imports(config, cv, force, false, &mut visited, 0)?;
    }

    // Resolve all crates including imports (reads from manifest cache, not config)
    let all_cratevars = imports::resolve_cratevars_with_imports(config, cratelist)?;

    let mut has_host_commands = false;
    for cv in &all_cratevars {
        let manifest = shimlink::load_cached_manifest(config, cv)?;
        if !manifest.manifest.host_commands.is_empty() {
            has_host_commands = true;
        }
        shimlink::create_shimlink_dir(&manifest, &shimdir)?;
    }

    let shimdir_str = shimdir.to_string_lossy().to_string();

    let path = if strict {
        if !has_host_commands {
            eprintln!("Note: Strict mode active with no host_commands. Only crate commands are on PATH.");
        }
        shimdir_str.clone()
    } else {
        let current_path = std::env::var("PATH").unwrap_or_default();
        format!("{}:{}", shimdir_str, current_path)
    };

    Ok(ActivationResult { path, shimdir: shimdir_str })
}

/// Determine the shell type from a shell path.
fn shell_type(shell_path: &str) -> &str {
    if shell_path.ends_with("zsh") {
        "zsh"
    } else {
        "bash"
    }
}

/// Check if a command is callable.
fn is_callable(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Build the crate display name for the prompt.
fn crate_display_name(cratelist: &[CrateVars]) -> String {
    cratelist
        .iter()
        .map(|cv| format!("{}/{}", cv.namespace, cv.crate_name))
        .collect::<Vec<_>>()
        .join(",")
}

/// Build the PS1 prompt string.
fn build_prompt(shell: &str, crate_name: &str, custom_prompt: Option<&str>) -> String {
    let template = match custom_prompt {
        Some(p) => p.to_string(),
        None => match shell {
            "zsh" => "%F{226}%b|%f%F{blue}%~%f %# ".to_string(),
            _ => r#"\[\033[01;93m\]\b|\[\033[00m\]\[\033[01;34m\]\w\[\033[00m\]\$ "#.to_string(),
        },
    };

    template
        .replace("\\b", crate_name)
        .replace("%b", crate_name)
}

/// Activate a crate environment by replacing the current process with a new shell.
pub fn activate(
    config: &BulkerConfig,
    config_path: Option<&Path>,
    cratelist: &[CrateVars],
    echo: bool,
    strict: bool,
    strict_env: bool,
    prompt: bool,
    force: bool,
) -> Result<()> {
    let result = get_new_path(config, cratelist, strict, force)?;
    let newpath = &result.path;
    let shimdir = &result.shimdir;
    // Use the first crate's display_name for BULKERCRATE (shimlink needs this to find the manifest)
    let crate_id = cratelist.first()
        .map(|cv| cv.display_name())
        .unwrap_or_default();
    let crate_name = crate_display_name(cratelist);

    // Resolve shell
    let shell_path = if !config.bulker.shell_path.is_empty() && is_callable(&config.bulker.shell_path) {
        config.bulker.shell_path.clone()
    } else if let Ok(shell) = std::env::var("SHELL") {
        if is_callable(&shell) {
            shell
        } else {
            "/bin/bash".to_string()
        }
    } else {
        "/bin/bash".to_string()
    };

    let shell = shell_type(&shell_path);

    // Resolve shell RC file
    let shell_rc = if !config.bulker.shell_rc.is_empty() {
        config.bulker.shell_rc.clone()
    } else if shell == "zsh" {
        std::env::var("HOME")
            .map(|h| format!("{}/.zshrc", h))
            .unwrap_or_else(|_| "$HOME/.zshrc".to_string())
    } else {
        std::env::var("HOME")
            .map(|h| format!("{}/.bashrc", h))
            .unwrap_or_else(|_| "$HOME/.bashrc".to_string())
    };

    // Build prompt
    let ps1 = build_prompt(shell, &crate_name, config.bulker.shell_prompt.as_deref());

    // Resolve rcfile paths from config directory (or default config path)
    let default_cfg = crate::config::default_config_path();
    let effective_config_path = config_path.unwrap_or(&default_cfg);
    let config_dir = config_templates_dir(effective_config_path);
    let rcfile = if strict {
        &config.bulker.rcfile_strict
    } else {
        &config.bulker.rcfile
    };
    let rcfile_path = config_dir.join(rcfile);

    // Echo mode: print export statements and return
    if echo {
        if std::env::var("BULKER_ORIG_PATH").is_err() {
            println!("export BULKER_ORIG_PATH=\"$PATH\"");
        }
        println!("export BULKERCRATE=\"{}\"", crate_id);
        if let Some(cp) = config_path {
            println!("export BULKERCFG=\"{}\"", cp.display());
        }
        if strict_env {
            println!("export BULKER_STRICT_ENV=1");
        }
        println!("export BULKERPATH=\"{}\"", newpath);
        println!("export BULKER_SHIMDIR=\"{}\"", shimdir);
        if prompt {
            println!("export BULKERPROMPT=\"{}\"", ps1);
        }
        println!("export BULKERSHELLRC=\"{}\"", shell_rc);
        println!("export PATH=\"{}\"", newpath);
        return Ok(());
    }

    // Set environment for the new shell
    // SAFETY: called in the main thread before exec replaces the process
    unsafe {
        std::env::set_var("BULKERCRATE", &crate_id);
        if let Some(cp) = config_path {
            std::env::set_var("BULKERCFG", cp.to_string_lossy().as_ref());
        }
        if strict_env {
            std::env::set_var("BULKER_STRICT_ENV", "1");
        }
        std::env::set_var("BULKERPATH", newpath);
        std::env::set_var("BULKER_SHIMDIR", shimdir);
        if prompt {
            std::env::set_var("BULKERPROMPT", &ps1);
        }
        std::env::set_var("BULKERSHELLRC", &shell_rc);

        // In strict mode, preserve explicitly listed envvars
        if strict {
            for var in &config.bulker.envvars {
                if let Ok(val) = std::env::var(var) {
                    std::env::set_var(var, &val);
                }
            }
        }
    }

    // Build shell command
    let mut cmd = std::process::Command::new(&shell_path);

    match shell {
        "bash" => {
            cmd.arg("--noprofile");
            cmd.arg("--rcfile");
            cmd.arg(rcfile_path.to_string_lossy().as_ref());
        }
        "zsh" => {
            // Zsh uses ZDOTDIR to find .zshrc
            let zdotdir = if strict {
                config_dir.join("zsh_start_strict")
            } else {
                config_dir.join("zsh_start")
            };
            // SAFETY: called before exec, single-threaded at this point
            unsafe { std::env::set_var("ZDOTDIR", zdotdir.to_string_lossy().as_ref()); }
        }
        _ => {
            log::warn!("Unknown shell type '{}', proceeding without rcfile", shell);
        }
    }

    // Replace current process with the shell (never returns on success)
    let err = cmd.exec();
    bail!("Failed to exec shell: {}", err);
}

/// Get the directory to resolve rcfile paths from (the config file's parent directory).
pub(crate) fn config_templates_dir(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_templates_dir_resolves_relative_to_config_file() {
        let config_path = Path::new("/some/custom/path/bulker_config.yaml");
        let result = config_templates_dir(config_path);
        assert_eq!(result, PathBuf::from("/some/custom/path"));
    }

    #[test]
    fn test_config_templates_dir_rcfile_joins_correctly() {
        let config_path = Path::new("/home/user/Dropbox/env/bulker_config/zither.yaml");
        let dir = config_templates_dir(config_path);
        let rcfile_path = dir.join("templates/start.sh");
        assert_eq!(
            rcfile_path,
            PathBuf::from("/home/user/Dropbox/env/bulker_config/templates/start.sh")
        );
    }
}
