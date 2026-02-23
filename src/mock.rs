// src/mock.rs
//
// Core mock mode logic: render mock executables and recording shims for CI testing
// without requiring Docker or any container runtime.

use anyhow::{Context, Result};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tera::Tera;

use crate::config::BulkerConfig;
use crate::manifest::{Manifest, PackageCommand};
use crate::templates;

pub const MOCK_EXE_TEMPLATE: &str = include_str!("../templates/mock_executable.tera");
pub const MOCK_RECORDING_TEMPLATE: &str = include_str!("../templates/mock_recording_executable.tera");

/// Render a mock executable script for a single command.
pub fn render_mock_executable(command: &str) -> Result<String> {
    let mut tera = Tera::default();
    tera.add_raw_template("mock_executable", MOCK_EXE_TEMPLATE)
        .context("Failed to parse mock_executable template")?;

    let mut ctx = tera::Context::new();
    ctx.insert("command", command);

    tera.render("mock_executable", &ctx)
        .context("Failed to render mock_executable template")
}

/// Render a mock recording executable script for a single command.
pub fn render_mock_recording_executable(command: &str, real_executable: &str) -> Result<String> {
    let mut tera = Tera::default();
    tera.add_raw_template("mock_recording_executable", MOCK_RECORDING_TEMPLATE)
        .context("Failed to parse mock_recording_executable template")?;

    let mut ctx = tera::Context::new();
    ctx.insert("command", command);
    ctx.insert("real_executable", real_executable);

    tera.render("mock_recording_executable", &ctx)
        .context("Failed to render mock_recording_executable template")
}

/// Write a rendered script to a file and make it executable (mode 0755).
fn write_executable(path: &Path, content: &str) -> Result<()> {
    std::fs::write(path, content)
        .with_context(|| format!("Failed to write executable: {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

/// Load a crate with mock wrappers that replay from outputs.json.
///
/// Creates `crate_path/<command>` for each command in the manifest.
/// The mock executables are self-contained Python scripts that read
/// BULKER_MOCK_OUTPUTS at runtime.
pub fn load_mock_crate(
    manifest: &Manifest,
    crate_path: &Path,
) -> Result<()> {
    std::fs::create_dir_all(crate_path)
        .with_context(|| format!("Failed to create mock crate dir: {}", crate_path.display()))?;

    let mut count = 0;
    for pkg in &manifest.manifest.commands {
        let content = render_mock_executable(&pkg.command)?;
        let exe_path = crate_path.join(&pkg.command);
        write_executable(&exe_path, &content)?;
        log::debug!("Created mock executable: {}", exe_path.display());
        count += 1;
    }

    // Also create mock executables for host_commands (they should also be mocked)
    for host_cmd in &manifest.manifest.host_commands {
        let content = render_mock_executable(host_cmd)?;
        let exe_path = crate_path.join(host_cmd);
        write_executable(&exe_path, &content)?;
        log::debug!("Created mock host command: {}", exe_path.display());
        count += 1;
    }

    log::info!(
        "Created mock crate with {} commands at {}",
        count,
        crate_path.display()
    );
    Ok(())
}

/// Load a crate with recording wrappers that capture outputs.
///
/// For each command, generates the real docker shim as `_real_<command>`,
/// then creates a recording wrapper as `<command>` that delegates to the
/// real shim while capturing stdout/stderr/returncode/new files.
pub fn load_recording_crate(
    manifest: &Manifest,
    config: &BulkerConfig,
    crate_path: &Path,
) -> Result<()> {
    std::fs::create_dir_all(crate_path)
        .with_context(|| format!("Failed to create recording crate dir: {}", crate_path.display()))?;

    let exe_template = templates::get_exe_template(config);
    let is_apptainer = config.bulker.container_engine == "apptainer";
    let mut count = 0;

    for pkg in &manifest.manifest.commands {
        // Render the real docker/apptainer shim
        let real_content = render_real_shim(config, exe_template, pkg, is_apptainer)?;
        let real_path = crate_path.join(format!("_real_{}", pkg.command));
        write_executable(&real_path, &real_content)?;
        log::debug!("Created real shim: {}", real_path.display());

        // Render the recording wrapper
        let recording_content = render_mock_recording_executable(
            &pkg.command,
            &real_path.to_string_lossy(),
        )?;
        let exe_path = crate_path.join(&pkg.command);
        write_executable(&exe_path, &recording_content)?;
        log::debug!("Created recording executable: {}", exe_path.display());
        count += 1;
    }

    // For host commands, the "real" executable is the host binary itself
    for host_cmd in &manifest.manifest.host_commands {
        if let Ok(output) = std::process::Command::new("which").arg(host_cmd).output() {
            if output.status.success() {
                let host_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let recording_content = render_mock_recording_executable(host_cmd, &host_path)?;
                let exe_path = crate_path.join(host_cmd);
                write_executable(&exe_path, &recording_content)?;
                log::debug!("Created recording host command: {}", exe_path.display());
                count += 1;
            } else {
                log::warn!("Host command not found for recording: {}", host_cmd);
            }
        }
    }

    log::info!(
        "Created recording crate with {} commands at {}",
        count,
        crate_path.display()
    );
    Ok(())
}

/// Render the real container shim for a package command (docker or apptainer).
fn render_real_shim(
    config: &BulkerConfig,
    exe_template: &str,
    pkg: &PackageCommand,
    is_apptainer: bool,
) -> Result<String> {
    if is_apptainer {
        use crate::manifest::parse_docker_image_path;
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
            "",
            &apptainer_image,
            &apptainer_fullpath,
        )
    } else {
        templates::render_template(exe_template, "executable", config, pkg, "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_mock_executable_contains_command() {
        let result = render_mock_executable("samtools").unwrap();
        assert!(result.contains("COMMAND = \"samtools\""));
        assert!(result.contains("BULKER_MOCK_OUTPUTS"));
        assert!(result.contains("#!/usr/bin/env python3"));
    }

    #[test]
    fn test_render_mock_recording_executable_contains_command_and_real() {
        let result = render_mock_recording_executable("samtools", "/tmp/crate/_real_samtools").unwrap();
        assert!(result.contains("COMMAND = \"samtools\""));
        assert!(result.contains("REAL_EXECUTABLE = \"/tmp/crate/_real_samtools\""));
        assert!(result.contains("BULKER_MOCK_RECORD_FILE"));
        assert!(result.contains("#!/usr/bin/env python3"));
    }
}
