use anyhow::Result;
use clap::{Arg, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};
use crate::crate_ops::{get_crate_path, load_crate, load_imports};
use crate::manifest::{CrateVars, load_remote_manifest};

pub fn create_cli() -> Command {
    Command::new("reload")
        .about("Re-fetch and rebuild all loaded crates from their manifests")
        .after_help("\
EXAMPLES:
  bulkers reload                          # reload all crates")
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .help("Bulker configuration file"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let mut config = BulkerConfig::from_file(&config_path)?;

    // Collect all loaded crates first (to avoid borrow issues)
    let mut to_reload: Vec<CrateVars> = Vec::new();
    for (namespace, crate_map) in config.crates().clone() {
        for (crate_name, tag_map) in crate_map {
            for (tag, _path) in tag_map {
                to_reload.push(CrateVars {
                    namespace: namespace.clone(),
                    crate_name: crate_name.clone(),
                    tag: tag.clone(),
                });
            }
        }
    }

    for cratevars in &to_reload {
        let registry_path = cratevars.display_name();
        log::info!("Reloading: {}", registry_path);

        match load_remote_manifest(&config, &registry_path, None) {
            Ok((manifest, cv)) => {
                let crate_path = get_crate_path(&config, &cv);

                if !manifest.manifest.imports.is_empty() {
                    load_imports(&manifest, &mut config, &config_path, false, true)?;
                }

                if let Err(e) = load_crate(&manifest, &cv, &mut config, &crate_path, false, true) {
                    log::warn!("Failed to reload {}: {}", registry_path, e);
                }
            }
            Err(e) => {
                log::warn!("Failed to fetch manifest for {}: {}", registry_path, e);
            }
        }
    }

    config.write(&config_path)?;
    println!("Reload complete. {} crates processed.", to_reload.len());
    Ok(())
}
