use anyhow::{Result, bail};
use std::collections::HashSet;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use crate::config::BulkerConfig;
use crate::crate_ops::get_local_path;
use crate::manifest::{CrateVars, parse_registry_path};

/// Resolve a crate's path and all its imports recursively (depth-first).
fn resolve_crate_paths(
    config: &BulkerConfig,
    cratevars: &CrateVars,
    visited: &mut HashSet<String>,
) -> Result<Vec<String>> {
    let display = cratevars.display_name();
    if visited.contains(&display) {
        return Ok(Vec::new());
    }
    visited.insert(display.clone());

    let mut paths = Vec::new();

    // Add the crate's own path first
    let path = get_local_path(config, cratevars)
        .ok_or_else(|| anyhow::anyhow!(
            "Crate '{}' is not installed. Run 'bulkers crate list' to see installed crates, or 'bulkers crate install' to add one.",
            display
        ))?;
    paths.push(path);

    // Resolve imports recursively
    if let Some(entry) = config.get_crate_entry(cratevars) {
        for import in &entry.imports {
            let import_cv = parse_registry_path(import, &config.bulker.default_namespace);
            let import_paths = resolve_crate_paths(config, &import_cv, visited)?;
            paths.extend(import_paths);
        }
    }

    Ok(paths)
}

/// Build the new PATH from a list of crates, resolving imports.
pub fn get_new_path(config: &BulkerConfig, cratelist: &[CrateVars], strict: bool) -> Result<String> {
    let mut all_paths = Vec::new();
    let mut visited = HashSet::new();

    for cv in cratelist {
        let paths = resolve_crate_paths(config, cv, &mut visited)?;
        all_paths.extend(paths);
    }

    let crate_path_str = all_paths.join(":");

    if strict {
        Ok(crate_path_str)
    } else {
        let current_path = std::env::var("PATH").unwrap_or_default();
        Ok(format!("{}:{}", crate_path_str, current_path))
    }
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
    config_path: &Path,
    cratelist: &[CrateVars],
    echo: bool,
    strict: bool,
    prompt: bool,
) -> Result<()> {
    let newpath = get_new_path(config, cratelist, strict)?;
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

    // Resolve rcfile paths from config directory
    let config_dir = config_templates_dir(config_path);
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
        println!("export BULKERCRATE=\"{}\"", crate_name);
        println!("export BULKERPATH=\"{}\"", newpath);
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
        std::env::set_var("BULKERCRATE", &crate_name);
        std::env::set_var("BULKERPATH", &newpath);
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
