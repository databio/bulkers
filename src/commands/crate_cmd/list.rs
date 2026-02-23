use anyhow::Result;
use clap::{ArgAction, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};

pub fn create_cli() -> Command {
    Command::new("list")
        .about("List installed crates")
        .after_help("\
EXAMPLES:
  bulkers crate list
  bulkers crate list --simple                   # simple format for scripting")
        .arg(
            clap::Arg::new("simple")
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
        println!("No crates installed.");
        return Ok(());
    }

    let mut entries = Vec::new();
    for (namespace, crate_map) in crates {
        for (crate_name, tag_map) in crate_map {
            for (tag, entry) in tag_map {
                entries.push((
                    format!("{}/{}:{}", namespace, crate_name, tag),
                    entry.path.clone(),
                    entry.imports.clone(),
                ));
            }
        }
    }

    if simple {
        let names: Vec<&str> = entries.iter().map(|(n, _, _)| n.as_str()).collect();
        println!("{}", names.join(" "));
    } else {
        for (name, path, imports) in &entries {
            if imports.is_empty() {
                println!("{} -- {}", name, path);
            } else {
                println!("{} -- {} (imports: {})", name, path, imports.join(", "));
            }
        }
    }

    Ok(())
}
