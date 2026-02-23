use anyhow::Result;
use clap::{ArgMatches, Command};

use crate::config::select_config;

pub fn create_cli() -> Command {
    Command::new("show")
        .about("Display current configuration")
        .after_help("\
EXAMPLES:
  bulkers config show")
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let contents = std::fs::read_to_string(&config_path)?;
    println!("{}", contents);
    Ok(())
}
