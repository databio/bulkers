use anyhow::{Result, bail};
use clap::{Arg, ArgAction, ArgMatches, Command};
use std::path::PathBuf;

use crate::config::{BulkerConfig, cache_config_to_disk, detect_engine, expand_path, resolve_engine_path};

pub fn create_cli() -> Command {
    Command::new("init")
        .about("Initialize (or reset) bulker configuration")
        .after_help("\
EXAMPLES:
  bulker config init
  bulker config init -c ~/.config/bulker/bulker_config.yaml
  bulker config init -c ~/bulker_config.yaml -e apptainer
  bulker config init --force                               # overwrite existing config")
        .arg(
            Arg::new("engine")
                .short('e')
                .long("engine")
                .value_parser(["docker", "apptainer"])
                .help("Container engine to use (auto-detected if not specified)"),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .action(ArgAction::SetTrue)
                .help("Overwrite existing config file"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = if let Some(c) = matches.get_one::<String>("config") {
        PathBuf::from(expand_path(c))
    } else {
        crate::config::default_config_path()
    };

    let force = matches.get_flag("force");

    // Check if config already exists (unless --force)
    if config_path.exists() && !force {
        bail!(
            "Config already exists at {}. Use --force to overwrite, or use a different path with -c.",
            config_path.display()
        );
    }

    // Build config from defaults with auto-detection
    let mut config = BulkerConfig::default();

    // Override engine if explicitly specified
    if let Some(e) = matches.get_one::<String>("engine") {
        config.bulker.container_engine = e.clone();
        config.bulker.engine_path = resolve_engine_path(e);
        // Engine-specific configuration is determined at runtime from container_engine
    } else if detect_engine().is_none() {
        bail!("No container engine found. Install docker or apptainer, or specify with --engine.");
    }

    // Write config and templates to disk
    cache_config_to_disk(&config, &config_path)?;

    log::info!("Initialized bulker config at: {}", config_path.display());
    log::info!("Container engine: {}", config.bulker.container_engine);
    if let Some(ref ep) = config.bulker.engine_path {
        log::info!("Engine path: {}", ep);
    }
    println!("Bulker config initialized at: {}", config_path.display());

    Ok(())
}
