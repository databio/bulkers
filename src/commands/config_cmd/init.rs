use anyhow::{Context, Result, bail};
use clap::{Arg, ArgMatches, Command};
use std::path::PathBuf;

use crate::config::{BulkerConfig, expand_path};
use crate::templates;

pub fn create_cli() -> Command {
    Command::new("init")
        .about("Initialize a new bulker configuration")
        .after_help("\
EXAMPLES:
  bulkers config init
  bulkers config init -c ~/.config/bulker/bulker_config.yaml
  bulkers config init -c ~/bulker_config.yaml -e apptainer")
        .arg(
            Arg::new("engine")
                .short('e')
                .long("engine")
                .value_parser(["docker", "apptainer"])
                .help("Container engine to use (auto-detected if not specified)"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = if let Some(c) = matches.get_one::<String>("config") {
        PathBuf::from(expand_path(c))
    } else {
        crate::config::default_config_path()
    };

    // Check if config already exists
    if config_path.exists() {
        bail!(
            "Config already exists at {}. Delete it first or use a different path with -c.",
            config_path.display()
        );
    }

    // Auto-detect container engine
    let engine = if let Some(e) = matches.get_one::<String>("engine") {
        e.clone()
    } else {
        detect_engine()?
    };

    // Create config directory
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
    }

    // Write templates to config directory
    let templates_dir = config_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("templates");
    templates::write_templates_to_dir(&templates_dir)?;

    // Build config from default template
    let mut config: BulkerConfig = serde_yaml::from_str(templates::DEFAULT_CONFIG)
        .context("Failed to parse default config template")?;

    // Set engine and resolve its absolute path
    config.bulker.container_engine = engine.clone();
    config.bulker.engine_path = resolve_engine_path(&engine);
    match engine.as_str() {
        "apptainer" => {
            config.bulker.executable_template = "apptainer_executable.tera".to_string();
            config.bulker.shell_template = "apptainer_shell.tera".to_string();
            config.bulker.build_template = "apptainer_build.tera".to_string();
        }
        _ => {
            config.bulker.executable_template = "docker_executable.tera".to_string();
            config.bulker.shell_template = "docker_shell.tera".to_string();
            config.bulker.build_template = "docker_build.tera".to_string();
        }
    }

    // Set rcfile paths relative to templates dir
    config.bulker.rcfile = "start.sh".to_string();
    config.bulker.rcfile_strict = "start_strict.sh".to_string();

    // Write config
    config.write(&config_path)?;

    log::info!("Initialized bulker config at: {}", config_path.display());
    log::info!("Container engine: {}", engine);
    if let Some(ref ep) = config.bulker.engine_path {
        log::info!("Engine path: {}", ep);
    }
    log::info!("Templates written to: {}", templates_dir.display());
    println!("Bulker config initialized at: {}", config_path.display());

    Ok(())
}

fn detect_engine() -> Result<String> {
    if is_in_path("docker") {
        Ok("docker".to_string())
    } else if is_in_path("apptainer") {
        Ok("apptainer".to_string())
    } else {
        bail!("No container engine found. Install docker or apptainer, or specify with --engine.");
    }
}

/// Resolve the absolute path of a command using `which`.
/// Returns Some(path) if found, None otherwise.
fn resolve_engine_path(engine: &str) -> Option<String> {
    std::process::Command::new("which")
        .arg(engine)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn is_in_path(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
