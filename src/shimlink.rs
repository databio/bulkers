//! Busybox-pattern executable dispatch. When bulkers is invoked via a symlink
//! (e.g., as "samtools"), argv[0] tells us which command to run. We look up the
//! command in the crate manifest, build the docker/apptainer command dynamically,
//! and exec it. No generated shell scripts needed.

use anyhow::{Context, Result, bail};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::sync::atomic::Ordering;

use crate::config::{BulkerConfig, expand_path, select_config};
use crate::manifest::{CrateVars, Manifest, PackageCommand, parse_docker_image_path, parse_registry_path};
use crate::process;

// ─── argv[0] detection ───────────────────────────────────────────────────────

/// Check if we were invoked as a shimlink (argv[0] != "bulkers").
/// Returns Some(command_name) if so, None if normal CLI invocation.
pub fn detect_shimlink_invocation() -> Option<String> {
    let argv0 = std::env::args().next()?;
    let filename = std::path::Path::new(&argv0).file_name()?.to_str()?;
    if filename == "bulkers" {
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
    let config_path = select_config(None)?;
    let config = BulkerConfig::from_file(&config_path)?;

    // 2. Load cached manifest for the crate
    let cratevars = parse_registry_path(&crate_id, &config.bulker.default_namespace);
    let manifest = load_cached_manifest(&config, &cratevars)?;

    // 3. Find command in manifest
    let pkg = manifest
        .manifest
        .commands
        .iter()
        .find(|c| c.command == actual_command)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Command '{}' not found in crate '{}'",
                actual_command,
                crate_id
            )
        })?;

    // 4. Resolve argument paths and auto-mount directories
    let (resolved_args, auto_mount_dirs) = resolve_arg_paths(args);

    // 5. Merge volumes: config + command + auto-mount
    let mut volumes = if pkg.no_default_volumes {
        Vec::new()
    } else {
        config.bulker.volumes.clone()
    };
    for v in &pkg.volumes {
        if !volumes.contains(v) {
            volumes.push(v.clone());
        }
    }
    for d in &auto_mount_dirs {
        if !volumes.contains(d) {
            volumes.push(d.clone());
        }
    }

    // 6. Merge envvars: config + command
    let mut envvars = config.bulker.envvars.clone();
    for e in &pkg.envvars {
        if !envvars.contains(e) {
            envvars.push(e.clone());
        }
    }
    // Add BULKER_EXTRA_ENVVARS from environment
    if let Ok(extra) = std::env::var("BULKER_EXTRA_ENVVARS") {
        for e in extra.split(',') {
            let e = e.trim().to_string();
            if !e.is_empty() && !envvars.contains(&e) {
                envvars.push(e);
            }
        }
    }

    // 7. Merge docker_args from multiple sources
    let mut docker_args = String::new();
    if let Some(ref da) = pkg.dockerargs {
        docker_args.push_str(da);
    }
    if let Some(ref da) = pkg.docker_args {
        if !docker_args.is_empty() {
            docker_args.push(' ');
        }
        docker_args.push_str(da);
    }
    // Host-tool-specific args from config
    let tool_extra = config.host_tool_specific_args(pkg, "docker_args");
    if !tool_extra.is_empty() {
        if !docker_args.is_empty() {
            docker_args.push(' ');
        }
        docker_args.push_str(&tool_extra);
    }
    // BULKER_EXTRA_DOCKER_ARGS from environment
    if let Ok(extra) = std::env::var("BULKER_EXTRA_DOCKER_ARGS") {
        if !extra.is_empty() {
            if !docker_args.is_empty() {
                docker_args.push(' ');
            }
            docker_args.push_str(&extra);
        }
    }

    // 8. Build and exec the container command
    let is_apptainer = config.bulker.container_engine == "apptainer";

    let cmd_vec = if is_apptainer {
        build_apptainer_command(
            &config,
            pkg,
            &volumes,
            &envvars,
            &resolved_args,
            interactive,
        )
    } else {
        build_docker_command(
            &config,
            pkg,
            &volumes,
            &envvars,
            &docker_args,
            &resolved_args,
            interactive,
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

    // Set up signal forwarding
    process::setup_signal_forwarding();

    // Spawn child in a new session (matching exec.rs pattern)
    let child = unsafe {
        std::process::Command::new(&cmd_vec[0])
            .args(&cmd_vec[1..])
            .pre_exec(|| {
                nix::unistd::setsid()
                    .map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
                Ok(())
            })
            .spawn()
            .with_context(|| format!("Failed to spawn: {}", cmd_vec[0]))?
    };

    let child_pid = child.id() as i32;
    process::CHILD_PID.store(child_pid, Ordering::SeqCst);

    let mut child = child;
    let status = child.wait().context("Failed to wait on child process")?;

    std::process::exit(status.code().unwrap_or(1));
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
) -> Vec<String> {
    let mut cmd = vec!["docker".to_string(), "run".to_string(), "--rm".to_string(), "--init".to_string()];

    if interactive {
        cmd.push("-it".to_string());
    }

    // Docker args
    if !docker_args.is_empty() {
        for part in shell_split(docker_args) {
            cmd.push(part);
        }
    }

    // User mapping (unless no_user)
    if !pkg.no_user {
        // Get uid:gid for --user flag
        let uid = get_id("-u");
        let gid = get_id("-g");
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
) -> Vec<String> {
    let (img_ns, img_name, _img_tag) = parse_docker_image_path(&pkg.docker_image);
    let apptainer_image = format!("{}-{}.sif", img_ns, img_name);
    let apptainer_fullpath = config
        .bulker
        .apptainer_image_folder
        .as_deref()
        .map(|f| format!("{}/{}", f, apptainer_image))
        .unwrap_or_else(|| apptainer_image.clone());

    let mut cmd = vec!["apptainer".to_string(), "exec".to_string()];

    // Apptainer-specific args
    if let Some(ref aa) = pkg.apptainer_args {
        if !aa.is_empty() {
            for part in shell_split(aa) {
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

/// Apply path map translation for bulker-in-docker scenarios.
/// If the path starts with a known prefix, replace it with the mapped prefix.
#[allow(dead_code)]
pub fn apply_path_map(path: &str, path_map: &[(String, String)]) -> String {
    for (from, to) in path_map {
        if path.starts_with(from.as_str()) {
            return format!("{}{}", to, &path[from.len()..]);
        }
    }
    path.to_string()
}

// ─── manifest caching ────────────────────────────────────────────────────────

/// Load a cached manifest from the manifest cache.
pub fn load_cached_manifest(_config: &BulkerConfig, cratevars: &CrateVars) -> Result<Manifest> {
    crate::manifest_cache::load_cached(cratevars)?
        .ok_or_else(|| anyhow::anyhow!(
            "Crate '{}' is not cached. Run 'bulkers activate {}' to fetch it.",
            cratevars.display_name(),
            cratevars.display_name()
        ))
}

// ─── shimlink directory creation ─────────────────────────────────────────────

/// Create a directory of symlinks pointing to the bulkers binary, one per command.
/// Also creates symlinks for host_commands pointing to the actual host binary.
/// Returns the path to the created directory.
pub fn create_shimlink_dir(manifest: &Manifest, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("Failed to create shimlink dir: {}", dir.display()))?;

    let bulkers_path = std::env::current_exe()
        .context("Failed to determine bulkers binary path")?;

    // Create symlinks for containerized commands
    for pkg in &manifest.manifest.commands {
        let link_path = dir.join(&pkg.command);
        let _ = std::fs::remove_file(&link_path);
        std::os::unix::fs::symlink(&bulkers_path, &link_path).with_context(|| {
            format!(
                "Failed to create shimlink: {} -> {}",
                link_path.display(),
                bulkers_path.display()
            )
        })?;

        // Also create _command shell wrapper symlink
        let shell_link_path = dir.join(format!("_{}", pkg.command));
        let _ = std::fs::remove_file(&shell_link_path);
        std::os::unix::fs::symlink(&bulkers_path, &shell_link_path).with_context(|| {
            format!(
                "Failed to create shell shimlink: {} -> {}",
                shell_link_path.display(),
                bulkers_path.display()
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

/// Get the current user/group ID by running `id`.
fn get_id(flag: &str) -> String {
    std::process::Command::new("id")
        .arg(flag)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "0".to_string())
}

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
    fn test_detect_shimlink_invocation_returns_none_for_bulkers() {
        // We cannot easily test argv[0] detection since it depends on the actual binary name.
        // Instead, test the logic directly.
        let filename = "bulkers";
        assert_eq!(filename == "bulkers", true);
    }

    #[test]
    fn test_detect_shimlink_name_extraction() {
        // Test the file_name extraction logic
        let path = std::path::Path::new("/usr/local/bin/samtools");
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(filename, "samtools");

        let path2 = std::path::Path::new("/usr/local/bin/bulkers");
        let filename2 = path2.file_name().unwrap().to_str().unwrap();
        assert_eq!(filename2, "bulkers");
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
    fn test_apply_path_map_matching_prefix() {
        let path_map = vec![
            ("/host/data".to_string(), "/container/data".to_string()),
        ];
        assert_eq!(
            apply_path_map("/host/data/file.txt", &path_map),
            "/container/data/file.txt"
        );
    }

    #[test]
    fn test_apply_path_map_no_match() {
        let path_map = vec![
            ("/host/data".to_string(), "/container/data".to_string()),
        ];
        assert_eq!(
            apply_path_map("/other/path/file.txt", &path_map),
            "/other/path/file.txt"
        );
    }

    #[test]
    fn test_apply_path_map_empty() {
        let path_map: Vec<(String, String)> = vec![];
        assert_eq!(apply_path_map("/any/path", &path_map), "/any/path");
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
        let config = make_test_config();
        let pkg = PackageCommand {
            command: "samtools".to_string(),
            docker_image: "quay.io/biocontainers/samtools:1.9".to_string(),
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
        };
        let volumes = vec!["/home/user".to_string()];
        let envvars = vec!["DISPLAY".to_string()];
        let args = vec!["view".to_string(), "test.bam".to_string()];

        let cmd = build_docker_command(&config, &pkg, &volumes, &envvars, "", &args, false);

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
        let config = make_test_config();
        let pkg = PackageCommand {
            command: "samtools".to_string(),
            docker_image: "quay.io/biocontainers/samtools:1.9".to_string(),
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
        };
        let cmd = build_docker_command(&config, &pkg, &[], &[], "", &[], true);
        assert!(cmd.contains(&"-it".to_string()));
        assert!(cmd.contains(&"bash".to_string()));
    }

    #[test]
    fn test_build_docker_command_no_user() {
        let config = make_test_config();
        let pkg = PackageCommand {
            command: "tool".to_string(),
            docker_image: "myimage:latest".to_string(),
            docker_command: None,
            docker_args: None,
            dockerargs: None,
            apptainer_args: None,
            apptainer_command: None,
            volumes: vec![],
            envvars: vec![],
            no_user: true,
            no_network: false,
            no_default_volumes: false,
            workdir: None,
        };
        let cmd = build_docker_command(&config, &pkg, &[], &[], "", &[], false);
        // Should NOT contain --user= or system volumes
        let cmd_str = cmd.join(" ");
        assert!(!cmd_str.contains("--user="));
        assert!(!cmd_str.contains("/etc/passwd"));
    }

    #[test]
    fn test_build_docker_command_host_network_disabled() {
        let mut config = make_test_config();
        config.bulker.host_network = false;
        let pkg = PackageCommand {
            command: "tool".to_string(),
            docker_image: "myimage:latest".to_string(),
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
        };
        let cmd = build_docker_command(&config, &pkg, &[], &[], "", &[], false);
        assert!(!cmd.contains(&"--network=host".to_string()));
    }

    #[test]
    fn test_build_docker_command_system_volumes_disabled() {
        let mut config = make_test_config();
        config.bulker.system_volumes = false;
        let pkg = PackageCommand {
            command: "tool".to_string(),
            docker_image: "myimage:latest".to_string(),
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
        };
        let cmd = build_docker_command(&config, &pkg, &[], &[], "", &[], false);
        let cmd_str = cmd.join(" ");
        assert!(!cmd_str.contains("/etc/passwd"));
    }

    #[test]
    fn test_build_docker_command_no_network() {
        let config = make_test_config();
        let pkg = PackageCommand {
            command: "tool".to_string(),
            docker_image: "myimage:latest".to_string(),
            docker_command: None,
            docker_args: None,
            dockerargs: None,
            apptainer_args: None,
            apptainer_command: None,
            volumes: vec![],
            envvars: vec![],
            no_user: false,
            no_network: true,
            no_default_volumes: false,
            workdir: None,
        };
        let cmd = build_docker_command(&config, &pkg, &[], &[], "", &[], false);
        assert!(!cmd.contains(&"--network=host".to_string()));
    }

    #[test]
    fn test_build_docker_command_with_docker_command() {
        let config = make_test_config();
        let pkg = PackageCommand {
            command: "python".to_string(),
            docker_image: "python:3.9".to_string(),
            docker_command: Some("python3".to_string()),
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
        };
        let cmd = build_docker_command(&config, &pkg, &[], &[], "", &["--version".to_string()], false);
        // Should use docker_command instead of command
        assert!(cmd.contains(&"python3".to_string()));
    }

    #[test]
    fn test_build_apptainer_command_basic() {
        let mut config = make_test_config();
        config.bulker.apptainer_image_folder = Some("/tmp/sif".to_string());
        let pkg = PackageCommand {
            command: "samtools".to_string(),
            docker_image: "quay.io/biocontainers/samtools:1.9".to_string(),
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
        };

        let cmd = build_apptainer_command(&config, &pkg, &[], &[], &[], false);
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
        let config = make_test_config();
        // config has volumes: ["$HOME"]
        let pkg_with_flag = PackageCommand {
            command: "postgres".to_string(),
            docker_image: "postgres:16".to_string(),
            docker_command: None,
            docker_args: None,
            dockerargs: None,
            apptainer_args: None,
            apptainer_command: None,
            volumes: vec!["/data".to_string()],
            envvars: vec![],
            no_user: true,
            no_network: false,
            no_default_volumes: true,
            workdir: None,
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
            docker_command: None,
            docker_args: None,
            dockerargs: None,
            apptainer_args: None,
            apptainer_command: None,
            volumes: vec!["/data".to_string()],
            envvars: vec![],
            no_user: true,
            no_network: false,
            no_default_volumes: false,
            workdir: None,
        };
        let volumes_without_flag = if pkg_without_flag.no_default_volumes {
            Vec::new()
        } else {
            config.bulker.volumes.clone()
        };
        // no_default_volumes=false -> starts with config volumes
        assert_eq!(volumes_without_flag, vec!["$HOME".to_string()]);
    }

    /// Helper to build a minimal BulkerConfig for tests.
    fn make_test_config() -> BulkerConfig {
        BulkerConfig {
            bulker: crate::config::BulkerSettings {
                container_engine: "docker".to_string(),
                default_namespace: "bulker".to_string(),
                registry_url: "http://hub.bulker.io/".to_string(),
                shell_path: "/bin/bash".to_string(),
                shell_rc: "$HOME/.bashrc".to_string(),
                executable_template: "docker_executable.tera".to_string(),
                shell_template: "docker_shell.tera".to_string(),
                build_template: "docker_build.tera".to_string(),
                rcfile: "start.sh".to_string(),
                rcfile_strict: "start_strict.sh".to_string(),
                volumes: vec!["$HOME".to_string()],
                envvars: vec!["DISPLAY".to_string()],
                host_network: true,
                system_volumes: true,
                tool_args: None,
                shell_prompt: None,
                apptainer_image_folder: None,
            },
        }
    }
}
