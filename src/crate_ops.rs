use anyhow::{Context, Result, bail};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::config::{BulkerConfig, CrateEntry, mkabs};
use crate::manifest::{
    CrateVars, Manifest, PackageCommand, load_remote_manifest, parse_docker_image_path,
};
use crate::templates;

/// Get the local filesystem path for a crate.
pub fn get_crate_path(config: &BulkerConfig, cratevars: &CrateVars) -> PathBuf {
    let base = mkabs(&config.bulker.default_crate_folder, None);
    base.join(&cratevars.namespace)
        .join(&cratevars.crate_name)
        .join(&cratevars.tag)
}

/// Get the stored local path for a crate from the config.
pub fn get_local_path(config: &BulkerConfig, cratevars: &CrateVars) -> Option<String> {
    crate::imports::get_crate_entry(config, cratevars).map(|entry| entry.path.clone())
}

/// Look up host-tool-specific arguments from the config's tool_args.
fn host_tool_specific_args(
    config: &BulkerConfig,
    pkg: &PackageCommand,
    arg_key: &str,
) -> String {
    let tool_args = match &config.bulker.tool_args {
        Some(v) => v,
        None => return String::new(),
    };

    // Parse docker image to get namespace/image/tag
    let (img_ns, img_name, img_tag) = parse_docker_image_path(&pkg.docker_image);

    // Try specific tag, then "default"
    for tag in &[img_tag.as_str(), "default"] {
        if let Some(val) = tool_args
            .get(&img_ns)
            .and_then(|ns| ns.get(&img_name))
            .and_then(|img| img.get(*tag))
            .and_then(|t| t.get(arg_key))
            .and_then(|v| v.as_str())
        {
            return val.to_string();
        }
    }

    String::new()
}

/// Load a crate from a manifest, rendering executable scripts.
pub fn load_crate(
    manifest: &Manifest,
    cratevars: &CrateVars,
    config: &mut BulkerConfig,
    crate_path: &Path,
    build: bool,
    force: bool,
) -> Result<()> {
    let crate_path_str = crate_path.to_string_lossy().to_string();

    // Check if already loaded
    if let Some(existing) = get_local_path(config, cratevars) {
        if Path::new(&existing).exists() && !force {
            bail!(
                "Crate '{}' already installed at {}. Use --force to overwrite.",
                cratevars.display_name(),
                existing
            );
        }
    }

    // Create crate directory
    std::fs::create_dir_all(crate_path)
        .with_context(|| format!("Failed to create crate dir: {}", crate_path.display()))?;

    let exe_template = templates::get_exe_template(config);
    let shell_template = templates::get_shell_template(config);
    let build_template = templates::get_build_template(config);

    let is_apptainer = config.bulker.container_engine == "apptainer";
    let mut commands_created = 0;

    for pkg in &manifest.manifest.commands {
        let extra_args = host_tool_specific_args(config, pkg, "docker_args");

        // Render executable
        let exe_content = if is_apptainer {
            let (img_ns, img_name, _img_tag) = parse_docker_image_path(&pkg.docker_image);
            let apptainer_image = format!("{}-{}.sif", img_ns, img_name);
            let apptainer_fullpath = config
                .bulker
                .apptainer_image_folder
                .as_deref()
                .map(|f| format!("{}/{}", f, apptainer_image))
                .unwrap_or_else(|| apptainer_image.clone());

            templates::render_template_apptainer(
                exe_template,
                "executable",
                config,
                pkg,
                &extra_args,
                &apptainer_image,
                &apptainer_fullpath,
            )?
        } else {
            templates::render_template(exe_template, "executable", config, pkg, &extra_args)?
        };

        // Write executable
        let exe_path = crate_path.join(&pkg.command);
        std::fs::write(&exe_path, &exe_content)
            .with_context(|| format!("Failed to write executable: {}", exe_path.display()))?;
        std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755))?;
        log::debug!("Created executable: {}", exe_path.display());

        // Render shell wrapper (prefixed with _)
        let shell_content = if is_apptainer {
            let (img_ns, img_name, _img_tag) = parse_docker_image_path(&pkg.docker_image);
            let apptainer_image = format!("{}-{}.sif", img_ns, img_name);
            let apptainer_fullpath = config
                .bulker
                .apptainer_image_folder
                .as_deref()
                .map(|f| format!("{}/{}", f, apptainer_image))
                .unwrap_or_else(|| apptainer_image.clone());

            templates::render_template_apptainer(
                shell_template,
                "shell",
                config,
                pkg,
                &extra_args,
                &apptainer_image,
                &apptainer_fullpath,
            )?
        } else {
            templates::render_template(shell_template, "shell", config, pkg, &extra_args)?
        };

        let shell_path = crate_path.join(format!("_{}", pkg.command));
        std::fs::write(&shell_path, &shell_content)?;
        std::fs::set_permissions(&shell_path, std::fs::Permissions::from_mode(0o755))?;
        log::debug!("Created shell wrapper: {}", shell_path.display());

        // Optionally build (pull) the image
        if build {
            let build_content = if is_apptainer {
                let (img_ns, img_name, _img_tag) = parse_docker_image_path(&pkg.docker_image);
                let apptainer_image = format!("{}-{}.sif", img_ns, img_name);
                let apptainer_fullpath = config
                    .bulker
                    .apptainer_image_folder
                    .as_deref()
                    .map(|f| format!("{}/{}", f, apptainer_image))
                    .unwrap_or_else(|| apptainer_image.clone());

                templates::render_template_apptainer(
                    build_template,
                    "build",
                    config,
                    pkg,
                    &extra_args,
                    &apptainer_image,
                    &apptainer_fullpath,
                )?
            } else {
                templates::render_template(build_template, "build", config, pkg, &extra_args)?
            };

            log::info!("Building image for: {}", pkg.command);
            let status = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(&build_content)
                .status()
                .context("Failed to run build script")?;
            if !status.success() {
                log::warn!("Build script failed for: {}", pkg.command);
            }
        }

        commands_created += 1;
    }

    // Handle host_commands (symlink host binaries into crate)
    for host_cmd in &manifest.manifest.host_commands {
        if let Ok(output) = std::process::Command::new("which")
            .arg(host_cmd)
            .output()
        {
            if output.status.success() {
                let host_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let link_path = crate_path.join(host_cmd);
                // Remove existing file/link
                let _ = std::fs::remove_file(&link_path);
                std::os::unix::fs::symlink(&host_path, &link_path)
                    .with_context(|| {
                        format!("Failed to symlink host command: {} -> {}", host_cmd, host_path)
                    })?;
                log::debug!("Symlinked host command: {} -> {}", host_cmd, host_path);
                commands_created += 1;
            } else {
                log::warn!("Host command not found: {}", host_cmd);
            }
        }
    }

    if commands_created == 0 {
        // Remove the empty crate directory
        let _ = std::fs::remove_dir_all(crate_path);
        bail!("No commands created for crate '{}'", cratevars.display_name());
    }

    // Update config crates map
    config
        .crates_mut()
        .entry(cratevars.namespace.clone())
        .or_default()
        .entry(cratevars.crate_name.clone())
        .or_default()
        .insert(cratevars.tag.clone(), CrateEntry {
            path: crate_path_str,
            imports: manifest.manifest.imports.clone(),
        });

    log::info!(
        "Installed crate '{}' with {} commands at {}",
        cratevars.display_name(),
        commands_created,
        crate_path.display()
    );

    Ok(())
}

/// Handle imports from a manifest. Always recurses.
pub fn load_imports(
    manifest: &Manifest,
    config: &mut BulkerConfig,
    config_path: &Path,
    build: bool,
) -> Result<()> {
    for import_path in &manifest.manifest.imports {
        log::info!("Installing import: {}", import_path);

        let (import_manifest, import_cratevars) =
            load_remote_manifest(config, import_path, None)?;

        let import_crate_path = get_crate_path(config, &import_cratevars);

        // Recursively handle imports first
        if !import_manifest.manifest.imports.is_empty() {
            load_imports(&import_manifest, config, config_path, build)?;
        }

        // load_crate stores imports from the manifest automatically
        load_crate(
            &import_manifest,
            &import_cratevars,
            config,
            &import_crate_path,
            build,
            true, // force for imports
        )?;
    }
    Ok(())
}

/// Unload a crate: remove from config and delete from disk.
pub fn unload_crate(config: &mut BulkerConfig, cratevars: &CrateVars) -> Result<()> {
    let path = get_local_path(config, cratevars);

    // Remove from config
    if let Some(ns) = config.crates_mut().get_mut(&cratevars.namespace) {
        if let Some(cr) = ns.get_mut(&cratevars.crate_name) {
            cr.remove(&cratevars.tag);
            // Clean up empty parents
            if cr.is_empty() {
                ns.remove(&cratevars.crate_name);
            }
        }
        if ns.is_empty() {
            config.crates_mut().remove(&cratevars.namespace);
        }
    }

    // Remove from disk
    if let Some(p) = path {
        let path = PathBuf::from(p);
        if path.exists() {
            std::fs::remove_dir_all(&path)
                .with_context(|| format!("Failed to remove crate dir: {}", path.display()))?;
            log::info!("Removed crate directory: {}", path.display());

            // Clean up empty parent dirs
            if let Some(parent) = path.parent() {
                let _ = std::fs::remove_dir(parent); // ignore error if not empty
                if let Some(grandparent) = parent.parent() {
                    let _ = std::fs::remove_dir(grandparent);
                }
            }
        }
    }

    log::info!("Uninstalled crate: {}", cratevars.display_name());
    Ok(())
}
