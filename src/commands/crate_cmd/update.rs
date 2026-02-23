use anyhow::Result;
use clap::{Arg, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};
use crate::crate_ops::{get_crate_path, load_crate, load_imports};
use crate::manifest::{CrateVars, load_remote_manifest, parse_registry_paths};

pub fn create_cli() -> Command {
    Command::new("update")
        .about("Re-fetch and rebuild installed crate(s) from their manifests")
        .after_help("\
EXAMPLES:
  bulkers crate update                          # update all crates
  bulkers crate update bulker/demo              # update a single crate")
        .arg(
            Arg::new("crate_registry_paths")
                .help("Crate to update (updates all if omitted)"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let mut config = BulkerConfig::from_file(&config_path)?;

    let to_reload: Vec<CrateVars> = if let Some(paths) = matches.get_one::<String>("crate_registry_paths") {
        parse_registry_paths(paths, &config.bulker.default_namespace)
    } else {
        // Collect all loaded crates
        let mut all = Vec::new();
        for (namespace, crate_map) in config.crates().clone() {
            for (crate_name, tag_map) in crate_map {
                for (tag, _entry) in tag_map {
                    all.push(CrateVars {
                        namespace: namespace.clone(),
                        crate_name: crate_name.clone(),
                        tag: tag.clone(),
                    });
                }
            }
        }
        all
    };

    for cratevars in &to_reload {
        let registry_path = cratevars.display_name();
        log::info!("Updating: {}", registry_path);

        match load_remote_manifest(&config, &registry_path, None) {
            Ok((manifest, cv)) => {
                let crate_path = get_crate_path(&config, &cv);

                if !manifest.manifest.imports.is_empty() {
                    load_imports(&manifest, &mut config, &config_path, false)?;
                }

                if let Err(e) = load_crate(&manifest, &cv, &mut config, &crate_path, false, true) {
                    log::warn!("Failed to update {}: {}", registry_path, e);
                }

                // Update import references
                if !manifest.manifest.imports.is_empty() {
                    if let Some(entry) = config.get_crate_entry_mut(&cv) {
                        entry.imports = manifest.manifest.imports.clone();
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to fetch manifest for {}: {}", registry_path, e);
            }
        }
    }

    config.write(&config_path)?;
    println!("Update complete. {} crates processed.", to_reload.len());
    Ok(())
}
