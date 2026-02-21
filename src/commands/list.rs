use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};

pub fn create_cli() -> Command {
    Command::new("list")
        .about("List loaded crates")
        .after_help("\
EXAMPLES:
  bulkers list
  bulkers list -s                         # simple format for scripting")
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .help("Bulker configuration file"),
        )
        .arg(
            Arg::new("simple")
                .short('s')
                .long("simple")
                .action(ArgAction::SetTrue)
                .help("Simple output format (space-separated)"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let config = BulkerConfig::from_file(&config_path)?;

    let simple = matches.get_flag("simple");
    let crates = config.crates();

    if crates.is_empty() {
        println!("No crates loaded.");
        return Ok(());
    }

    let mut entries = Vec::new();
    for (namespace, crate_map) in crates {
        for (crate_name, tag_map) in crate_map {
            for (tag, path) in tag_map {
                entries.push((format!("{}/{}:{}", namespace, crate_name, tag), path.clone()));
            }
        }
    }

    if simple {
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        println!("{}", names.join(" "));
    } else {
        for (name, path) in &entries {
            println!("{} -- {}", name, path);
        }
    }

    Ok(())
}
