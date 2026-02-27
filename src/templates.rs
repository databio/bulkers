//! Tera template rendering for container commands (docker/apptainer).
//! Three template types: executable (shimlink invocations), shell (interactive
//! `_command` variants), and build (`crate install --build` image pulls).
//! Template selection is based on `config.bulker.container_engine`, not the
//! legacy template name fields in config (which exist for serialization but are not read).

use anyhow::{Context, Result};
use std::path::Path;
use tera::Tera;

use crate::config::BulkerConfig;
use crate::manifest::PackageCommand;
pub const DOCKER_EXE_TEMPLATE: &str = include_str!("../templates/docker_executable.tera");
pub const DOCKER_SHELL_TEMPLATE: &str = include_str!("../templates/docker_shell.tera");
pub const DOCKER_BUILD_TEMPLATE: &str = include_str!("../templates/docker_build.tera");
pub const APPTAINER_EXE_TEMPLATE: &str = include_str!("../templates/apptainer_executable.tera");
pub const APPTAINER_SHELL_TEMPLATE: &str = include_str!("../templates/apptainer_shell.tera");
pub const APPTAINER_BUILD_TEMPLATE: &str = include_str!("../templates/apptainer_build.tera");

pub const BASH_RC: &str = include_str!("../templates/start.sh");
pub const BASH_RC_STRICT: &str = include_str!("../templates/start_strict.sh");
pub const ZSH_RC: &str = include_str!("../templates/zsh_start/.zshrc");
pub const ZSH_RC_STRICT: &str = include_str!("../templates/zsh_start_strict/.zshrc");
/// Write all embedded templates to a directory on disk (for rcfile references).
pub fn write_templates_to_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("Failed to create templates dir: {}", dir.display()))?;

    let files = [
        ("docker_executable.tera", DOCKER_EXE_TEMPLATE),
        ("docker_shell.tera", DOCKER_SHELL_TEMPLATE),
        ("docker_build.tera", DOCKER_BUILD_TEMPLATE),
        ("apptainer_executable.tera", APPTAINER_EXE_TEMPLATE),
        ("apptainer_shell.tera", APPTAINER_SHELL_TEMPLATE),
        ("apptainer_build.tera", APPTAINER_BUILD_TEMPLATE),
        ("start.sh", BASH_RC),
        ("start_strict.sh", BASH_RC_STRICT),
    ];

    for (name, content) in &files {
        std::fs::write(dir.join(name), content)
            .with_context(|| format!("Failed to write template: {}", name))?;
    }

    // Zsh rcfiles need subdirectories
    let zsh_dir = dir.join("zsh_start");
    std::fs::create_dir_all(&zsh_dir)?;
    std::fs::write(zsh_dir.join(".zshrc"), ZSH_RC)?;

    let zsh_strict_dir = dir.join("zsh_start_strict");
    std::fs::create_dir_all(&zsh_strict_dir)?;
    std::fs::write(zsh_strict_dir.join(".zshrc"), ZSH_RC_STRICT)?;

    Ok(())
}

/// Build a Tera context from a PackageCommand merged with config-level settings.
fn build_context(
    config: &BulkerConfig,
    pkg: &PackageCommand,
    extra_docker_args: &str,
) -> tera::Context {
    let mut ctx = tera::Context::new();

    // Merge volumes: config-level + command-level
    let mut volumes = config.bulker.volumes.clone();
    crate::manifest::merge_lists(&mut volumes, &pkg.volumes);

    // Auto-mount temp directory ($TMPDIR or /tmp)
    let tmpdir = crate::shimlink::tmpdir_volume();
    if !volumes.contains(&tmpdir) {
        volumes.push(tmpdir);
    }

    ctx.insert("volumes", &volumes);

    // Merge envvars: config-level + command-level
    let mut envvars = config.bulker.envvars.clone();
    crate::manifest::merge_lists(&mut envvars, &pkg.envvars);
    ctx.insert("envvars", &envvars);

    ctx.insert("engine_path", config.engine_path());
    ctx.insert("docker_image", &pkg.docker_image);
    ctx.insert("command", &pkg.command);
    ctx.insert("no_user", &pkg.no_user);
    ctx.insert("no_network", &pkg.no_network);

    // Docker-specific
    ctx.insert("docker_command", &pkg.docker_command.as_deref().unwrap_or(""));
    ctx.insert("workdir", &pkg.workdir.as_deref().unwrap_or(""));

    // Merge docker_args from multiple sources
    let all_docker_args = pkg.merged_docker_args(&[extra_docker_args]);
    if all_docker_args.is_empty() {
        ctx.insert("dockerargs", &"");
        ctx.insert("docker_args", &"");
    } else {
        ctx.insert("dockerargs", &all_docker_args);
        ctx.insert("docker_args", &"");
    }

    // Apptainer-specific
    ctx.insert("apptainer_args", &pkg.apptainer_args.as_deref().unwrap_or(""));
    ctx.insert("apptainer_command", &pkg.apptainer_command.as_deref().unwrap_or(""));

    ctx
}

/// Render an executable script from a template string.
pub fn render_template(
    template_content: &str,
    template_name: &str,
    config: &BulkerConfig,
    pkg: &PackageCommand,
    extra_docker_args: &str,
) -> Result<String> {
    let mut tera = Tera::default();
    tera.add_raw_template(template_name, template_content)
        .with_context(|| format!("Failed to parse template: {}", template_name))?;

    let ctx = build_context(config, pkg, extra_docker_args);
    tera.render(template_name, &ctx)
        .with_context(|| format!("Failed to render template: {}", template_name))
}

/// Render an executable script with apptainer-specific context added.
pub fn render_template_apptainer(
    template_content: &str,
    template_name: &str,
    config: &BulkerConfig,
    pkg: &PackageCommand,
    extra_docker_args: &str,
    apptainer_image: &str,
    apptainer_fullpath: &str,
) -> Result<String> {
    let mut tera = Tera::default();
    tera.add_raw_template(template_name, template_content)
        .with_context(|| format!("Failed to parse template: {}", template_name))?;

    let mut ctx = build_context(config, pkg, extra_docker_args);
    ctx.insert("apptainer_image", apptainer_image);
    ctx.insert("apptainer_fullpath", apptainer_fullpath);

    tera.render(template_name, &ctx)
        .with_context(|| format!("Failed to render template: {}", template_name))
}

/// Get the executable template content for the configured engine.
pub fn get_exe_template(config: &BulkerConfig) -> &'static str {
    if config.bulker.container_engine == "apptainer" {
        APPTAINER_EXE_TEMPLATE
    } else {
        DOCKER_EXE_TEMPLATE
    }
}

/// Get the build template content for the configured engine.
pub fn get_build_template(config: &BulkerConfig) -> &'static str {
    if config.bulker.container_engine == "apptainer" {
        APPTAINER_BUILD_TEMPLATE
    } else {
        DOCKER_BUILD_TEMPLATE
    }
}
