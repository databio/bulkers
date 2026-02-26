use anyhow::{Context, Result};
use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config::load_config;

pub fn create_cli() -> Command {
    Command::new("show")
        .about("Display current configuration")
        .after_help("\
EXAMPLES:
  bulker config show
  bulker config show --effective")
        .arg(
            Arg::new("effective")
                .long("effective")
                .action(ArgAction::SetTrue)
                .help("Show effective config (file merged with defaults)"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let (config, config_path) = load_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;

    if matches.get_flag("effective") {
        let yaml = serde_yml::to_string(&config)
            .context("Failed to serialize config")?;
        println!("{}", yaml);
    } else {
        let config_path = config_path.context("No config file exists. Use --effective to see built-in defaults.")?;
        let contents = std::fs::read_to_string(&config_path)?;
        println!("{}", contents);
    }
    Ok(())
}
