use anyhow::Result;
use clap::{Arg, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};
use crate::crate_ops::unload_crate;
use crate::manifest::parse_registry_paths;

pub fn create_cli() -> Command {
    Command::new("uninstall")
        .about("Remove an installed crate from disk and config")
        .after_help("\
EXAMPLES:
  bulkers crate uninstall bulker/demo
  bulkers crate uninstall databio/pepatac:1.0.13")
        .arg(
            Arg::new("crate_registry_paths")
                .required(true)
                .help("Crate to remove, e.g. bulker/demo or namespace/crate:tag"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let mut config = BulkerConfig::from_file(&config_path)?;

    let registry_paths = matches.get_one::<String>("crate_registry_paths").unwrap();
    let cratelist = parse_registry_paths(registry_paths, &config.bulker.default_namespace);

    for cratevars in &cratelist {
        unload_crate(&mut config, cratevars)?;
        println!("Uninstalled: {}", cratevars.display_name());
    }

    config.write(&config_path)?;
    Ok(())
}
