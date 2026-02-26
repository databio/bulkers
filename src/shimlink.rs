//! Busybox-pattern executable dispatch. When bulker is invoked via a symlink
//! (e.g., as "samtools"), argv[0] tells us which command to run. We look up the
//! command in the crate manifest, build the docker/apptainer command dynamically,
//! and exec it. No generated shell scripts needed.

use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::config::{BulkerConfig, expand_path, load_config};
use crate::manifest::{CrateVars, Manifest, PackageCommand, parse_registry_path};
use crate::process;

// ─── argv[0] detection ───────────────────────────────────────────────────────

/// Check if we were invoked as a shimlink (argv[0] != "bulker").
/// Returns Some(command_name) if so, None if normal CLI invocation.
pub fn detect_shimlink_invocation() -> Option<String> {
    let argv0 = std::env::args().next()?;
    let filename = std::path::Path::new(&argv0).file_name()?.to_str()?;
    if filename == "bulker" {
        None
    } else {
        Some(filename.to_string())
    }
}

// ─── shimlink execution ──────────────────────────────────────────────────────

/// Execute a command via shimlink dispatch.
/// Reads $BULKERCRATE and $BULKERCFG, looks up the command in the manifest,
/// constructs the docker/apptainer command, and exec()s it.
pub fn shimlink_exec(command_name: &str, args: &[String]) -> Result<()> {
    // Handle _command prefix for shell/interactive wrappers
    let (actual_command, interactive) = if command_name.starts_with('_') {
        (&command_name[1..], true)
    } else {
        (command_name, false)
    };

    // 1. Read environment
    let crate_id = std::env::var("BULKERCRATE")
        .context("$BULKERCRATE not set. Are you in an activated bulker environment?")?;
    let (config, _config_path) = load_config(None)?;

    // 2. Find command in crate manifest or its imports
    let cratevars = parse_registry_path(&crate_id, &config.bulker.default_namespace)?;
    let pkg = find_command_in_crate_with_imports(&config, &cratevars, actual_command)?;

    // 3. Resolve argument paths and auto-mount directories
    let (resolved_args, auto_mount_dirs) = resolve_arg_paths(args);

    // 4. Merge volumes: config + command + auto-mount
    let mut volumes = if pkg.no_default_volumes {
        Vec::new()
    } else {
        config.bulker.volumes.clone()
    };
    crate::manifest::merge_lists(&mut volumes, &pkg.volumes);
    crate::manifest::merge_lists(&mut volumes, &auto_mount_dirs);

    // 5. Merge envvars: config + command
    let mut envvars = config.bulker.envvars.clone();
    crate::manifest::merge_lists(&mut envvars, &pkg.envvars);
    // Add BULKER_EXTRA_ENVVARS from environment
    if let Ok(extra) = std::env::var("BULKER_EXTRA_ENVVARS") {
        let extras: Vec<String> = extra.split(',').map(|e| e.trim().to_string()).filter(|e| !e.is_empty()).collect();
        crate::manifest::merge_lists(&mut envvars, &extras);
    }

    // 6. Merge docker_args from multiple sources
    let tool_extra = config.host_tool_specific_args(&pkg, "docker_args");
    let env_extra = std::env::var("BULKER_EXTRA_DOCKER_ARGS").unwrap_or_default();
    let docker_args = pkg.merged_docker_args(&[&tool_extra, &env_extra]);

    // 7. Build and exec the container command
    let is_apptainer = config.bulker.container_engine == "apptainer";

    let engine_path = config.engine_path();

    let cmd_vec = if is_apptainer {
        build_apptainer_command(
            &config,
            &pkg,
            &volumes,
            &envvars,
            &resolved_args,
            interactive,
            engine_path,
        )
    } else {
        build_docker_command(
            &config,
            &pkg,
            &volumes,
            &envvars,
            &docker_args,
            &resolved_args,
            interactive,
            engine_path,
        )
    };

    if cmd_vec.is_empty() {
        bail!("Failed to build container command");
    }

    // Print command instead of executing if BULKER_PRINT_COMMAND is set
    if std::env::var("BULKER_PRINT_COMMAND").is_ok() {
        println!("{}", cmd_vec.join(" "));
        return Ok(());
    }

    log::debug!("Shimlink exec: {:?}", cmd_vec);

    let exit_code = process::spawn_and_wait(&cmd_vec[0], &cmd_vec[1..])?;

    std::process::exit(exit_code);
}

// ─── command construction ────────────────────────────────────────────────────

/// Build a docker run command from resolved command config.
pub fn build_docker_command(
    config: &BulkerConfig,
    pkg: &PackageCommand,
    volumes: &[String],
    envvars: &[String],
    docker_args: &str,
    args: &[String],
    interactive: bool,
    engine_path: &str,
) -> Vec<String> {
    let mut cmd = vec![engine_path.to_string(), "run".to_string(), "--rm".to_string(), "--init".to_string()];

    if interactive {
        cmd.push("-it".to_string());
    }

    // Docker args
    if !docker_args.is_empty() {
        let expanded_args = expand_path(docker_args);
        for part in shell_split(&expanded_args) {
            cmd.push(part);
        }
    }

    // User mapping (unless no_user)
    if !pkg.no_user {
        // Get uid:gid for --user flag
        let uid = nix::unistd::getuid();
        let gid = nix::unistd::getgid();
        cmd.push(format!("--user={}:{}", uid, gid));
    }

    // Network (unless no_network or config disables host networking)
    if !pkg.no_network && config.bulker.host_network {
        cmd.push("--network=host".to_string());
    }

    // Environment variables
    for envvar in envvars {
        cmd.push("--env".to_string());
        cmd.push(envvar.clone());
    }

    // Volume mounts
    for volume in volumes {
        let expanded = expand_path(volume);
        cmd.push("--volume".to_string());
        cmd.push(format!("{}:{}", expanded, expanded));
    }

    // System volumes for user mapping (skipped on macOS via config)
    if !pkg.no_user && config.bulker.system_volumes {
        for sys_vol in &[
            "/etc/group:/etc/group:ro",
            "/etc/passwd:/etc/passwd:ro",
            "/etc/shadow:/etc/shadow:ro",
            "/etc/sudoers.d:/etc/sudoers.d:ro",
            "/tmp/.X11-unix:/tmp/.X11-unix:rw",
        ] {
            cmd.push("--volume".to_string());
            cmd.push(sys_vol.to_string());
        }
    }

    // Working directory
    let workdir = match &pkg.workdir {
        Some(w) if !w.is_empty() => w.clone(),
        _ => std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "/".to_string()),
    };
    cmd.push(format!("--workdir={}", workdir));

    // Image
    cmd.push(pkg.docker_image.clone());

    // Command to run inside container
    if interactive {
        // Shell wrapper: launch bash
        cmd.push("bash".to_string());
    } else if let Some(ref dc) = pkg.docker_command {
        if !dc.is_empty() {
            cmd.push(dc.clone());
        }
    } else {
        cmd.push(pkg.command.clone());
    }

    // User arguments
    for arg in args {
        cmd.push(arg.clone());
    }

    cmd
}

/// Build an apptainer exec command from resolved command config.
pub fn build_apptainer_command(
    config: &BulkerConfig,
    pkg: &PackageCommand,
    volumes: &[String],
    _envvars: &[String],
    args: &[String],
    interactive: bool,
    engine_path: &str,
) -> Vec<String> {
    let (_, apptainer_fullpath) = crate::manifest::apptainer_image_paths(
        &pkg.docker_image,
        config.bulker.apptainer_image_folder.as_deref(),
    );

    let mut cmd = vec![engine_path.to_string(), "exec".to_string()];

    // Apptainer-specific args
    if let Some(ref aa) = pkg.apptainer_args {
        if !aa.is_empty() {
            let expanded_args = expand_path(aa);
            for part in shell_split(&expanded_args) {
                cmd.push(part);
            }
        }
    }

    // Volume binds (apptainer skips $HOME since it's auto-bound)
    for volume in volumes {
        let expanded = expand_path(volume);
        if expanded != expand_path("$HOME") && expanded != expand_path("${HOME}") {
            cmd.push("-B".to_string());
            cmd.push(format!("{}:{}", expanded, expanded));
        }
    }

    // Image path
    cmd.push(apptainer_fullpath);

    // Command to run
    if interactive {
        cmd.push("bash".to_string());
    } else if let Some(ref ac) = pkg.apptainer_command {
        if !ac.is_empty() {
            cmd.push(ac.clone());
        }
    } else if let Some(ref dc) = pkg.docker_command {
        if !dc.is_empty() {
            cmd.push(dc.clone());
        }
    } else {
        cmd.push(pkg.command.clone());
    }

    // User arguments
    for arg in args {
        cmd.push(arg.clone());
    }

    cmd
}

// ─── argument path resolution ────────────────────────────────────────────────

/// Resolve file-like arguments to absolute paths and collect parent directories for auto-mounting.
/// Returns (resolved_args, auto_mount_dirs).
pub fn resolve_arg_paths(args: &[String]) -> (Vec<String>, Vec<String>) {
    let mut resolved_args = Vec::with_capacity(args.len());
    let mut auto_mount_dirs = Vec::new();

    for arg in args {
        // Skip flags (start with -)
        if arg.starts_with('-') {
            resolved_args.push(arg.clone());
            continue;
        }

        let path = Path::new(arg);

        // If the path exists on the filesystem, resolve it to absolute
        if path.exists() {
            if let Ok(abs) = std::fs::canonicalize(path) {
                let abs_str = abs.to_string_lossy().to_string();

                // Add parent directory as auto-mount
                if let Some(parent) = abs.parent() {
                    let parent_str = parent.to_string_lossy().to_string();
                    if !auto_mount_dirs.contains(&parent_str) {
                        auto_mount_dirs.push(parent_str);
                    }
                }

                resolved_args.push(abs_str);
                continue;
            }
        }

        // Check if it looks like a path (contains / or .) and its parent exists
        if (arg.contains('/') || arg.contains('.')) && !arg.starts_with('-') {
            if let Some(parent) = path.parent() {
                if parent.exists() && !parent.as_os_str().is_empty() {
                    if let Ok(abs_parent) = std::fs::canonicalize(parent) {
                        let parent_str = abs_parent.to_string_lossy().to_string();
                        if !auto_mount_dirs.contains(&parent_str) {
                            auto_mount_dirs.push(parent_str);
                        }
                        // Resolve the arg with absolute parent + filename
                        if let Some(filename) = path.file_name() {
                            let abs_path = abs_parent.join(filename);
                            resolved_args.push(abs_path.to_string_lossy().to_string());
                            continue;
                        }
                    }
                }
            }
        }

        // Pass through unchanged
        resolved_args.push(arg.clone());
    }

    (resolved_args, auto_mount_dirs)
}

// ─── command lookup with imports ─────────────────────────────────────────────

/// Find a command by searching the primary crate manifest and all its imports.
fn find_command_in_crate_with_imports(
    config: &BulkerConfig,
    primary_cv: &CrateVars,
    command_name: &str,
) -> Result<PackageCommand> {
    let all_crates = crate::imports::resolve_cratevars_with_imports(config, &[primary_cv.clone()])?;

    for cv in &all_crates {
        if let Some(manifest) = crate::manifest_cache::load_cached(cv)? {
            if let Some(pkg) = manifest.manifest.commands.iter().find(|c| c.command == command_name) {
                return Ok(pkg.clone());
            }
        }
    }

    bail!(
        "Command '{}' not found in crate '{}' or its imports",
        command_name,
        primary_cv.display_name()
    )
}

// ─── manifest caching ────────────────────────────────────────────────────────

/// Load a cached manifest from the manifest cache.
pub fn load_cached_manifest(_config: &BulkerConfig, cratevars: &CrateVars) -> Result<Manifest> {
    crate::manifest_cache::load_cached(cratevars)?
        .ok_or_else(|| anyhow::anyhow!(
            "Crate '{}' is not cached. Run 'bulker activate {}' to fetch it.",
            cratevars.display_name(),
            cratevars.display_name()
        ))
}

// ─── shimlink directory creation ─────────────────────────────────────────────

/// Create a directory of symlinks pointing to the bulker binary, one per command.
/// Also creates symlinks for host_commands pointing to the actual host binary.
/// Returns the path to the created directory.
pub fn create_shimlink_dir(manifest: &Manifest, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("Failed to create shimlink dir: {}", dir.display()))?;

    let bulker_path = std::env::current_exe()
        .context("Failed to determine bulker binary path")?;

    // Create symlinks for containerized commands
    for pkg in &manifest.manifest.commands {
        let link_path = dir.join(&pkg.command);
        let _ = std::fs::remove_file(&link_path);
        std::os::unix::fs::symlink(&bulker_path, &link_path).with_context(|| {
            format!(
                "Failed to create shimlink: {} -> {}",
                link_path.display(),
                bulker_path.display()
            )
        })?;

        // Also create _command shell wrapper symlink
        let shell_link_path = dir.join(format!("_{}", pkg.command));
        let _ = std::fs::remove_file(&shell_link_path);
        std::os::unix::fs::symlink(&bulker_path, &shell_link_path).with_context(|| {
            format!(
                "Failed to create shell shimlink: {} -> {}",
                shell_link_path.display(),
                bulker_path.display()
            )
        })?;
    }

    // Symlink host commands to the actual host binary
    for host_cmd in &manifest.manifest.host_commands {
        if let Ok(output) = std::process::Command::new("which")
            .arg(host_cmd)
            .output()
        {
            if output.status.success() {
                let host_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let link_path = dir.join(host_cmd);
                let _ = std::fs::remove_file(&link_path);
                std::os::unix::fs::symlink(&host_path, &link_path).with_context(|| {
                    format!("Failed to symlink host command: {} -> {}", host_cmd, host_path)
                })?;
            } else {
                log::warn!("Host command not found: {}", host_cmd);
            }
        }
    }

    Ok(())
}

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Simple shell-like argument splitting (handles quoted strings).
fn shell_split(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escape_next = false;

    for ch in s.chars() {
        if escape_next {
            current.push(ch);
            escape_next = false;
            continue;
        }

        match ch {
            '\\' if !in_single_quote => {
                escape_next = true;
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            ' ' | '\t' if !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    result.push(current.clone());
                    current.clear();
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }

    if !current.is_empty() {
        result.push(current);
    }

    result
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ManifestInner, Manifest};

    #[test]
    fn test_detect_shimlink_invocation_returns_none_for_bulker() {
        // We cannot easily test argv[0] detection since it depends on the actual binary name.
        // Instead, test the logic directly.
        let filename = "bulker";
        assert_eq!(filename == "bulker", true);
    }

    #[test]
    fn test_detect_shimlink_name_extraction() {
        // Test the file_name extraction logic
        let path = std::path::Path::new("/usr/local/bin/samtools");
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(filename, "samtools");

        let path2 = std::path::Path::new("/usr/local/bin/bulker");
        let filename2 = path2.file_name().unwrap().to_str().unwrap();
        assert_eq!(filename2, "bulker");
    }

    #[test]
    fn test_shell_wrapper_prefix_detection() {
        let cmd = "_samtools";
        let (actual, interactive) = if cmd.starts_with('_') {
            (&cmd[1..], true)
        } else {
            (cmd, false)
        };
        assert_eq!(actual, "samtools");
        assert!(interactive);

        let cmd2 = "samtools";
        let (actual2, interactive2) = if cmd2.starts_with('_') {
            (&cmd2[1..], true)
        } else {
            (cmd2, false)
        };
        assert_eq!(actual2, "samtools");
        assert!(!interactive2);
    }

    #[test]
    fn test_resolve_arg_paths_flags_pass_through() {
        let args = vec!["--verbose".to_string(), "-n".to_string(), "5".to_string()];
        let (resolved, auto_mounts) = resolve_arg_paths(&args);
        assert_eq!(resolved, args);
        assert!(auto_mounts.is_empty());
    }

    #[test]
    fn test_resolve_arg_paths_existing_file() {
        // /tmp always exists
        let args = vec!["/tmp".to_string()];
        let (resolved, _auto_mounts) = resolve_arg_paths(&args);
        assert_eq!(resolved[0], "/tmp");
    }

    #[test]
    fn test_shell_split_simple() {
        let result = shell_split("--gpus all --shm-size 8g");
        assert_eq!(result, vec!["--gpus", "all", "--shm-size", "8g"]);
    }

    #[test]
    fn test_shell_split_quoted() {
        let result = shell_split("--label \"my label\" --name test");
        assert_eq!(result, vec!["--label", "my label", "--name", "test"]);
    }

    #[test]
    fn test_shell_split_single_quoted() {
        let result = shell_split("--env 'FOO=bar baz'");
        assert_eq!(result, vec!["--env", "FOO=bar baz"]);
    }

    #[test]
    fn test_build_docker_command_basic() {
        let config = BulkerConfig::test_default();
        let pkg = PackageCommand {
            command: "samtools".to_string(),
            docker_image: "quay.io/biocontainers/samtools:1.9".to_string(),
            ..Default::default()
        };
        let volumes = vec!["/home/user".to_string()];
        let envvars = vec!["DISPLAY".to_string()];
        let args = vec!["view".to_string(), "test.bam".to_string()];

        let cmd = build_docker_command(&config, &pkg, &volumes, &envvars, "", &args, false, "docker");

        assert_eq!(cmd[0], "docker");
        assert_eq!(cmd[1], "run");
        assert_eq!(cmd[2], "--rm");
        assert_eq!(cmd[3], "--init");
        // Should NOT contain -it for non-interactive
        assert!(!cmd.contains(&"-it".to_string()));
        // Should contain the image
        assert!(cmd.contains(&"quay.io/biocontainers/samtools:1.9".to_string()));
        // Should contain the command
        assert!(cmd.contains(&"samtools".to_string()));
        // Should contain user args
        assert!(cmd.contains(&"view".to_string()));
        assert!(cmd.contains(&"test.bam".to_string()));
        // Should contain --network=host
        assert!(cmd.contains(&"--network=host".to_string()));
    }

    #[test]
    fn test_build_docker_command_interactive() {
        let config = BulkerConfig::test_default();
        let pkg = PackageCommand {
            command: "samtools".to_string(),
            docker_image: "quay.io/biocontainers/samtools:1.9".to_string(),
            ..Default::default()
        };
        let cmd = build_docker_command(&config, &pkg, &[], &[], "", &[], true, "docker");
        assert!(cmd.contains(&"-it".to_string()));
        assert!(cmd.contains(&"bash".to_string()));
    }

    #[test]
    fn test_build_docker_command_no_user() {
        let config = BulkerConfig::test_default();
        let pkg = PackageCommand {
            command: "tool".to_string(),
            docker_image: "myimage:latest".to_string(),
            no_user: true,
            ..Default::default()
        };
        let cmd = build_docker_command(&config, &pkg, &[], &[], "", &[], false, "docker");
        // Should NOT contain --user= or system volumes
        let cmd_str = cmd.join(" ");
        assert!(!cmd_str.contains("--user="));
        assert!(!cmd_str.contains("/etc/passwd"));
    }

    #[test]
    fn test_build_docker_command_host_network_disabled() {
        let mut config = BulkerConfig::test_default();
        config.bulker.host_network = false;
        let pkg = PackageCommand {
            command: "tool".to_string(),
            docker_image: "myimage:latest".to_string(),
            ..Default::default()
        };
        let cmd = build_docker_command(&config, &pkg, &[], &[], "", &[], false, "docker");
        assert!(!cmd.contains(&"--network=host".to_string()));
    }

    #[test]
    fn test_build_docker_command_system_volumes_disabled() {
        let mut config = BulkerConfig::test_default();
        config.bulker.system_volumes = false;
        let pkg = PackageCommand {
            command: "tool".to_string(),
            docker_image: "myimage:latest".to_string(),
            ..Default::default()
        };
        let cmd = build_docker_command(&config, &pkg, &[], &[], "", &[], false, "docker");
        let cmd_str = cmd.join(" ");
        assert!(!cmd_str.contains("/etc/passwd"));
    }

    #[test]
    fn test_build_docker_command_no_network() {
        let config = BulkerConfig::test_default();
        let pkg = PackageCommand {
            command: "tool".to_string(),
            docker_image: "myimage:latest".to_string(),
            no_network: true,
            ..Default::default()
        };
        let cmd = build_docker_command(&config, &pkg, &[], &[], "", &[], false, "docker");
        assert!(!cmd.contains(&"--network=host".to_string()));
    }

    #[test]
    fn test_build_docker_command_with_docker_command() {
        let config = BulkerConfig::test_default();
        let pkg = PackageCommand {
            command: "python".to_string(),
            docker_image: "python:3.9".to_string(),
            docker_command: Some("python3".to_string()),
            ..Default::default()
        };
        let cmd = build_docker_command(&config, &pkg, &[], &[], "", &["--version".to_string()], false, "docker");
        // Should use docker_command instead of command
        assert!(cmd.contains(&"python3".to_string()));
    }

    #[test]
    fn test_build_docker_command_expands_env_in_docker_args() {
        let config = BulkerConfig::test_default();
        let pkg = PackageCommand {
            command: "R".to_string(),
            docker_image: "r-base:4.3".to_string(),
            ..Default::default()
        };
        let home = std::env::var("HOME").unwrap();
        let docker_args = "--volume=${HOME}/R/4.0:/usr/local/lib/R/host-site-library";
        let cmd = build_docker_command(&config, &pkg, &[], &[], docker_args, &[], false, "docker");
        let cmd_str = cmd.join(" ");
        // ${HOME} should be expanded, not passed literally
        assert!(!cmd_str.contains("${HOME}"), "env var not expanded: {}", cmd_str);
        assert!(cmd_str.contains(&format!("--volume={}/R/4.0:/usr/local/lib/R/host-site-library", home)));
    }

    #[test]
    fn test_build_apptainer_command_basic() {
        let mut config = BulkerConfig::test_default();
        config.bulker.apptainer_image_folder = Some("/tmp/sif".to_string());
        let pkg = PackageCommand {
            command: "samtools".to_string(),
            docker_image: "quay.io/biocontainers/samtools:1.9".to_string(),
            ..Default::default()
        };

        let cmd = build_apptainer_command(&config, &pkg, &[], &[], &[], false, "apptainer");
        assert_eq!(cmd[0], "apptainer");
        assert_eq!(cmd[1], "exec");
        // Should contain the SIF path
        assert!(cmd.iter().any(|a| a.contains(".sif")));
        // Should contain the command
        assert!(cmd.contains(&"samtools".to_string()));
    }

    #[test]
    fn test_create_shimlink_dir() {
        let manifest = Manifest {
            manifest: ManifestInner {
                name: Some("test".to_string()),
                version: None,
                commands: vec![
                    PackageCommand {
                        command: "samtools".to_string(),
                        docker_image: "samtools:latest".to_string(),
                        docker_command: None,
                        docker_args: None,
                        dockerargs: None,
                        apptainer_args: None,
                        apptainer_command: None,
                        volumes: vec![],
                        envvars: vec![],
                        no_user: false,
                        no_network: false,
                        no_default_volumes: false,
                        workdir: None,
                    },
                    PackageCommand {
                        command: "bcftools".to_string(),
                        docker_image: "bcftools:latest".to_string(),
                        docker_command: None,
                        docker_args: None,
                        dockerargs: None,
                        apptainer_args: None,
                        apptainer_command: None,
                        volumes: vec![],
                        envvars: vec![],
                        no_user: false,
                        no_network: false,
                        no_default_volumes: false,
                        workdir: None,
                    },
                ],
                host_commands: vec![],
                imports: vec![],
            },
        };

        let tmpdir = tempfile::tempdir().unwrap();
        let shimdir = tmpdir.path().join("shims");
        create_shimlink_dir(&manifest, &shimdir).unwrap();

        // Check that symlinks were created
        assert!(shimdir.join("samtools").exists());
        assert!(shimdir.join("bcftools").exists());
        assert!(shimdir.join("_samtools").exists());
        assert!(shimdir.join("_bcftools").exists());

        // All should be symlinks
        assert!(shimdir.join("samtools").is_symlink());
        assert!(shimdir.join("bcftools").is_symlink());
        assert!(shimdir.join("_samtools").is_symlink());
        assert!(shimdir.join("_bcftools").is_symlink());
    }

    #[test]
    fn test_no_default_volumes_skips_config_volumes() {
        let config = BulkerConfig::test_default();
        // config has volumes: ["$HOME"]
        let pkg_with_flag = PackageCommand {
            command: "postgres".to_string(),
            docker_image: "postgres:16".to_string(),
            volumes: vec!["/data".to_string()],
            no_user: true,
            no_default_volumes: true,
            ..Default::default()
        };

        // Simulate the volume merge logic from shimlink_exec
        let volumes_with_flag = if pkg_with_flag.no_default_volumes {
            Vec::new()
        } else {
            config.bulker.volumes.clone()
        };
        // no_default_volumes=true -> starts empty, only per-command volumes added
        assert!(volumes_with_flag.is_empty());

        let pkg_without_flag = PackageCommand {
            command: "postgres".to_string(),
            docker_image: "postgres:16".to_string(),
            volumes: vec!["/data".to_string()],
            no_user: true,
            ..Default::default()
        };
        let volumes_without_flag = if pkg_without_flag.no_default_volumes {
            Vec::new()
        } else {
            config.bulker.volumes.clone()
        };
        // no_default_volumes=false -> starts with config volumes
        assert_eq!(volumes_without_flag, vec!["$HOME".to_string()]);
    }

    #[test]
    fn test_engine_path_accessor_returns_absolute_when_set() {
        let mut config = BulkerConfig::test_default();
        config.bulker.engine_path = Some("/usr/bin/docker".to_string());
        assert_eq!(config.engine_path(), "/usr/bin/docker");
    }

    #[test]
    fn test_engine_path_accessor_falls_back_to_engine_name() {
        let config = BulkerConfig::test_default();
        assert_eq!(config.engine_path(), "docker");
    }

    #[test]
    fn test_build_docker_command_uses_engine_path() {
        let config = BulkerConfig::test_default();
        let pkg = PackageCommand {
            command: "samtools".to_string(),
            docker_image: "quay.io/biocontainers/samtools:1.9".to_string(),
            ..Default::default()
        };
        let cmd = build_docker_command(&config, &pkg, &[], &[], "", &[], false, "/usr/bin/docker");
        assert_eq!(cmd[0], "/usr/bin/docker");
    }

    #[test]
    fn test_build_apptainer_command_uses_engine_path() {
        let mut config = BulkerConfig::test_default();
        config.bulker.apptainer_image_folder = Some("/tmp/sif".to_string());
        let pkg = PackageCommand {
            command: "samtools".to_string(),
            docker_image: "quay.io/biocontainers/samtools:1.9".to_string(),
            ..Default::default()
        };
        let cmd = build_apptainer_command(&config, &pkg, &[], &[], &[], false, "/usr/local/bin/apptainer");
        assert_eq!(cmd[0], "/usr/local/bin/apptainer");
    }

    /// Helper to build a PackageCommand with default fields.
    fn make_empty_pkg() -> PackageCommand {
        PackageCommand::default()
    }

    #[test]
    fn test_find_command_in_imported_crate() {
        let tmpdir = tempfile::tempdir().unwrap();
        let _guard = crate::test_util::EnvGuard::set("XDG_CONFIG_HOME", tmpdir.path());

        let config = BulkerConfig::test_default();

        // Child crate: bulker/coreutils_shimtest with "cat" command
        let child_cv = CrateVars {
            namespace: "bulker".to_string(),
            crate_name: "coreutils_shimtest".to_string(),
            tag: "default".to_string(),
        };
        let child_manifest = Manifest {
            manifest: ManifestInner {
                name: Some("coreutils".to_string()),
                version: None,
                commands: vec![PackageCommand {
                    command: "cat".to_string(),
                    docker_image: "alpine:latest".to_string(),
                    ..make_empty_pkg()
                }],
                host_commands: vec![],
                imports: vec![],
            },
        };
        crate::manifest_cache::save_to_cache(&child_cv, &child_manifest).unwrap();

        // Parent crate: test/parent_shimtest that imports bulker/coreutils_shimtest
        let parent_cv = CrateVars {
            namespace: "test".to_string(),
            crate_name: "parent_shimtest".to_string(),
            tag: "default".to_string(),
        };
        let parent_manifest = Manifest {
            manifest: ManifestInner {
                name: Some("parent".to_string()),
                version: None,
                commands: vec![PackageCommand {
                    command: "samtools".to_string(),
                    docker_image: "samtools:latest".to_string(),
                    ..make_empty_pkg()
                }],
                host_commands: vec![],
                imports: vec!["bulker/coreutils_shimtest:default".to_string()],
            },
        };
        crate::manifest_cache::save_to_cache(&parent_cv, &parent_manifest).unwrap();

        // Look up "cat" starting from the parent crate — should find it in the import
        let pkg = find_command_in_crate_with_imports(&config, &parent_cv, "cat").unwrap();
        assert_eq!(pkg.command, "cat");

        // Also verify "samtools" is found in the primary crate
        let pkg2 = find_command_in_crate_with_imports(&config, &parent_cv, "samtools").unwrap();
        assert_eq!(pkg2.command, "samtools");

        // EnvGuard restores XDG_CONFIG_HOME on drop
    }

}
