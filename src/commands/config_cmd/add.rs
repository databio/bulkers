use anyhow::{Result, bail};
use clap::{Arg, ArgMatches, Command};

use anyhow::Context;
use crate::config::load_config;

pub fn create_cli() -> Command {
    Command::new("add")
        .about("Add an entry to a list config field")
        .after_help("\
EXAMPLES:
  bulker config add envvars DISPLAY
  bulker config add volumes /data")
        .arg(
            Arg::new("key")
                .required(true)
                .help("List field name (envvars, volumes)"),
        )
        .arg(
            Arg::new("value")
                .required(true)
                .help("Value to add to the list"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let (mut config, config_path) = load_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let config_path = config_path.context("No config file to write to. Run `bulker config init` first.")?;
    let key = matches.get_one::<String>("key").unwrap();
    let value = matches.get_one::<String>("value").unwrap();

    if !super::is_list_key(key) {
        bail!("'{}' is not a list field. Use 'config set' instead.", key);
    }

    let list = match key.as_str() {
        "envvars" => &mut config.bulker.envvars,
        "volumes" => &mut config.bulker.volumes,
        _ => unreachable!(),
    };

    if list.contains(value) {
        println!("'{}' already in {}", value, key);
        return Ok(());
    }

    list.push(value.clone());
    config.write(&config_path)?;
    println!("Added '{}' to {}", value, key);
    Ok(())
}
