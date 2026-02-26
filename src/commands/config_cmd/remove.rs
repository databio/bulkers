use anyhow::{Result, bail};
use clap::{Arg, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};

pub fn create_cli() -> Command {
    Command::new("remove")
        .about("Remove an entry from a list config field")
        .after_help("\
EXAMPLES:
  bulkers config remove envvars DISPLAY
  bulkers config remove volumes /data")
        .arg(
            Arg::new("key")
                .required(true)
                .help("List field name (envvars, volumes)"),
        )
        .arg(
            Arg::new("value")
                .required(true)
                .help("Value to remove from the list"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let mut config = BulkerConfig::from_file(&config_path)?;
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

    if let Some(pos) = list.iter().position(|v| v == value) {
        list.remove(pos);
        config.write(&config_path)?;
        println!("Removed '{}' from {}", value, key);
    } else {
        println!("'{}' not found in {}", value, key);
    }

    Ok(())
}
