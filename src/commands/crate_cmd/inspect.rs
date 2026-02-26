use anyhow::Result;
use clap::{Arg, ArgMatches, Command};

use crate::config::load_config;
use crate::manifest::parse_registry_paths;
use crate::manifest_cache;

pub fn create_cli() -> Command {
    Command::new("inspect")
        .about("Show commands available in a cached crate")
        .after_help("\
EXAMPLES:
  bulker crate inspect                         # inspect the currently active crate
  bulker crate inspect bulker/demo
  bulker crate inspect databio/pepatac:1.0.13")
        .arg(
            Arg::new("crate_registry_paths")
                .help("Crate to inspect (defaults to active crate from BULKERCRATE)"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let (config, _config_path) = load_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;

    let registry_path = match matches.get_one::<String>("crate_registry_paths") {
        Some(p) => p.clone(),
        None => std::env::var("BULKERCRATE")
            .map_err(|_| anyhow::anyhow!("No crate specified and no active crate (BULKERCRATE not set)"))?,
    };
    let cratelist = parse_registry_paths(&registry_path, &config.bulker.default_namespace)?;

    for cratevars in &cratelist {
        let manifest = manifest_cache::load_cached(cratevars)?
            .ok_or_else(|| anyhow::anyhow!(
                "Crate '{}' is not cached. Run 'bulker activate {}' to fetch it.",
                cratevars.display_name(), cratevars.display_name()
            ))?;

        println!("Crate: {}", cratevars.display_name());

        // Show digests
        let manifest_digest = crate::manifest_cache::ensure_crate_manifest_digest(cratevars)?;
        if let Some(ref d) = manifest_digest {
            println!("crate-manifest-digest:  {}", d);
        }
        let image_digest = crate::manifest_cache::read_digest_sidecar(cratevars, "crate-image-digest");
        if let Some(ref d) = image_digest {
            println!("crate-image-digest:     {}", d);
        } else {
            println!("crate-image-digest:     not available");
        }
        println!();

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

        // Show imports from the manifest itself
        if !manifest.manifest.imports.is_empty() {
            println!("Imports:");
            for import in &manifest.manifest.imports {
                let import_cv = crate::manifest::parse_registry_path(import, &config.bulker.default_namespace)?;
                match manifest_cache::load_cached(&import_cv) {
                    Ok(Some(m)) => {
                        let count = m.manifest.commands.len() + m.manifest.host_commands.len();
                        println!("  {} ({} commands)", import, count);
                    }
                    _ => println!("  {} (not cached)", import),
                }
            }
        }

        let total = commands.len() + manifest.manifest.host_commands.len();
        println!("\n{} commands available", total);
        println!();
    }

    Ok(())
}
