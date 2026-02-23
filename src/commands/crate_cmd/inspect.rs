use anyhow::{Result, bail};
use clap::{Arg, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};
use crate::crate_ops::get_local_path;
use crate::manifest::parse_registry_paths;
use crate::shimlink;

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

        // Load the cached manifest to list commands
        match shimlink::load_cached_manifest(&config, &cratevars) {
            Ok(manifest) => {
                let mut commands: Vec<&str> = manifest.manifest.commands.iter()
                    .map(|c| c.command.as_str())
                    .collect();
                commands.sort();

                println!("Commands:");
                for cmd in &commands {
                    println!("  {}", cmd);
                }

                if !manifest.manifest.host_commands.is_empty() {
                    println!("Host commands:");
                    for cmd in &manifest.manifest.host_commands {
                        println!("  {}", cmd);
                    }
                }

                let total = commands.len() + manifest.manifest.host_commands.len();
                println!("\n{} commands available", total);
            }
            Err(_) => {
                // Fall back to directory listing if no cached manifest
                let path = std::path::PathBuf::from(&crate_path);
                if path.is_dir() {
                    let mut commands: Vec<String> = std::fs::read_dir(&path)?
                        .filter_map(|e| e.ok())
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .filter(|name| !name.starts_with('_') && name != "manifest.yaml")
                        .collect();
                    commands.sort();

                    println!("Commands:");
                    for cmd in &commands {
                        println!("  {}", cmd);
                    }
                    println!("\n{} commands available", commands.len());
                }
            }
        }

        // Show resolved imports
        if let Some(entry) = config.get_crate_entry(&cratevars) {
            if !entry.imports.is_empty() {
                println!("Imports:");
                for import in &entry.imports {
                    let import_cratevars = crate::manifest::parse_registry_path(import, &config.bulker.default_namespace);
                    match shimlink::load_cached_manifest(&config, &import_cratevars) {
                        Ok(m) => {
                            let count = m.manifest.commands.len() + m.manifest.host_commands.len();
                            println!("  {} ({} commands)", import, count);
                        }
                        Err(_) => println!("  {} (not installed)", import),
                    }
                }
            }
        }

        println!();
    }

    Ok(())
}
