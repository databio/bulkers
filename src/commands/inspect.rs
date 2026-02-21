use anyhow::{Result, bail};
use clap::{Arg, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};
use crate::crate_ops::get_local_path;
use crate::manifest::parse_registry_paths;

pub fn create_cli() -> Command {
    Command::new("inspect")
        .about("View commands in a loaded crate")
        .after_help("\
EXAMPLES:
  bulkers inspect                         # inspect the currently active crate
  bulkers inspect bulker/demo
  bulkers inspect databio/pepatac:1.0.13")
        .arg(
            Arg::new("crate_registry_paths")
                .help("Crate to inspect (defaults to active crate from BULKERCRATE)"),
        )
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .help("Bulker configuration file"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let config = BulkerConfig::from_file(&config_path)?;

    let registry_path = match matches.get_one::<String>("crate_registry_paths") {
        Some(p) => p.clone(),
        None => std::env::var("BULKERCRATE")
            .map_err(|_| anyhow::anyhow!("No crate specified and no active crate (BULKERCRATE not set)"))?,
    };
    let cratelist = parse_registry_paths(&registry_path, &config.bulker.default_namespace);

    for cratevars in &cratelist {
        let crate_path = match get_local_path(&config, cratevars) {
            Some(p) => p,
            None => bail!("Crate '{}' is not loaded. Run 'bulkers list' to see loaded crates.", cratevars.display_name()),
        };

        println!("Crate: {}", cratevars.display_name());
        println!("Path:  {}", crate_path);
        println!("Commands:");

        let path = std::path::PathBuf::from(&crate_path);
        if path.is_dir() {
            let mut commands: Vec<String> = std::fs::read_dir(&path)?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|name| !name.starts_with('_'))
                .collect();
            commands.sort();

            for cmd in &commands {
                println!("  {}", cmd);
            }
            println!("\n{} commands available", commands.len());
        }
        println!();
    }

    Ok(())
}
