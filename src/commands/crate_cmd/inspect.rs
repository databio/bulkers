use anyhow::{Result, bail};
use clap::{Arg, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};
use crate::crate_ops::get_local_path;
use crate::manifest::parse_registry_paths;

pub fn create_cli() -> Command {
    Command::new("inspect")
        .about("Show commands available in an installed crate")
        .after_help("\
EXAMPLES:
  bulkers crate inspect                         # inspect the currently active crate
  bulkers crate inspect bulker/demo
  bulkers crate inspect databio/pepatac:1.0.13")
        .arg(
            Arg::new("crate_registry_paths")
                .help("Crate to inspect (defaults to active crate from BULKERCRATE)"),
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
        let crate_path = match get_local_path(&config, &cratevars) {
            Some(p) => p,
            None => bail!("Crate '{}' is not installed. Run 'bulkers crate list' to see installed crates.", cratevars.display_name()),
        };

        println!("Crate: {}", cratevars.display_name());
        println!("Path:  {}", crate_path);

        // Show the crate's own commands
        let path = std::path::PathBuf::from(&crate_path);
        if path.is_dir() {
            let mut commands: Vec<String> = std::fs::read_dir(&path)?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|name| !name.starts_with('_'))
                .collect();
            commands.sort();

            println!("Commands:");
            for cmd in &commands {
                println!("  {}", cmd);
            }
            println!("\n{} commands available", commands.len());
        }

        // Show resolved imports
        if let Some(entry) = config.get_crate_entry(&cratevars) {
            if !entry.imports.is_empty() {
                println!("\nImports:");
                for import in &entry.imports {
                    let import_cratevars = crate::manifest::parse_registry_path(import, &config.bulker.default_namespace);
                    let import_path = get_local_path(&config, &import_cratevars);
                    match import_path {
                        Some(p) => {
                            let ip = std::path::PathBuf::from(&p);
                            let count = if ip.is_dir() {
                                std::fs::read_dir(&ip)
                                    .map(|rd| rd.filter_map(|e| e.ok())
                                        .filter(|e| !e.file_name().to_string_lossy().starts_with('_'))
                                        .count())
                                    .unwrap_or(0)
                            } else {
                                0
                            };
                            println!("  {} ({} commands)", import, count);
                        }
                        None => println!("  {} (not installed)", import),
                    }
                }
            }
        }

        println!();
    }

    Ok(())
}
