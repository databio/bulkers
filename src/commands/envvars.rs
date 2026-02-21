use anyhow::Result;
use clap::{Arg, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};

pub fn create_cli() -> Command {
    Command::new("envvars")
        .about("Manage environment variables passed to containers")
        .after_help("\
EXAMPLES:
  bulkers envvars                         # list current variables
  bulkers envvars -a MY_VAR              # add a variable
  bulkers envvars -r MY_VAR              # remove a variable")
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .help("Bulker configuration file"),
        )
        .arg(
            Arg::new("add")
                .short('a')
                .long("add")
                .help("Add an environment variable"),
        )
        .arg(
            Arg::new("remove")
                .short('r')
                .long("remove")
                .help("Remove an environment variable"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let mut config = BulkerConfig::from_file(&config_path)?;

    if let Some(var) = matches.get_one::<String>("add") {
        if !config.bulker.envvars.contains(var) {
            config.bulker.envvars.push(var.clone());
            config.write(&config_path)?;
            println!("Added environment variable: {}", var);
        } else {
            println!("Environment variable already exists: {}", var);
        }
    } else if let Some(var) = matches.get_one::<String>("remove") {
        if let Some(pos) = config.bulker.envvars.iter().position(|v| v == var) {
            config.bulker.envvars.remove(pos);
            config.write(&config_path)?;
            println!("Removed environment variable: {}", var);
        } else {
            println!("Environment variable not found: {}", var);
        }
    } else {
        // List current envvars
        println!("Environment variables:");
        for var in &config.bulker.envvars {
            println!("  {}", var);
        }
    }

    Ok(())
}
