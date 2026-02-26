use anyhow::{Result, bail};
use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config::load_config;
use crate::manifest::parse_registry_paths;
use crate::manifest_cache;

pub fn create_cli() -> Command {
    Command::new("clean")
        .about("Remove cached crate manifests")
        .after_help("\
EXAMPLES:
  bulkers crate clean databio/pepatac:1.0.13    # remove a specific cached manifest
  bulkers crate clean --all                     # clear entire manifest cache")
        .arg(
            Arg::new("crate_registry_paths")
                .help("Crate(s) to clean (comma-separated for multiple)"),
        )
        .arg(
            Arg::new("all")
                .long("all")
                .action(ArgAction::SetTrue)
                .help("Clear the entire manifest cache"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    if matches.get_flag("all") {
        let base = manifest_cache::cache_base_dir();
        if base.exists() {
            std::fs::remove_dir_all(&base)?;
            println!("Cleared manifest cache: {}", base.display());
        } else {
            println!("Manifest cache is already empty.");
        }
    } else if let Some(registry_paths) = matches.get_one::<String>("crate_registry_paths") {
        let (config, _config_path) = load_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
        let cratelist = parse_registry_paths(registry_paths, &config.bulker.default_namespace);
        for cv in &cratelist {
            manifest_cache::remove_cached(cv)?;
            println!("Removed: {}", cv.display_name());
        }
    } else {
        bail!("Specify a crate to clean, or use --all to clear the entire cache.");
    }
    Ok(())
}
