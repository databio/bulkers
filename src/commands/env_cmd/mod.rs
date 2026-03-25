use anyhow::{Context, Result};
use clap::{Arg, ArgMatches, Command};

use crate::config::load_config;
use crate::shimlink::{DEFAULT_ENVVARS, expand_envvar_patterns};

pub fn create_cli() -> Command {
    Command::new("env")
        .about("Manage environment variable forwarding")
        .after_help("\
EXAMPLES:
  bulker env                      Show resolved forwarded vars
  bulker env add MY_CUSTOM_DB     Forward a host var by name
  bulker env add \"AWS_*\"          Forward all vars matching a prefix
  bulker env set LANG=C           Set a hardcoded value
  bulker env remove MY_CUSTOM_DB  Stop forwarding a var")
        .subcommand(
            Command::new("add")
                .about("Add a name or pattern to forwarded envvars")
                .arg(Arg::new("pattern").required(true).help("Var name or glob pattern (e.g. AWS_*)")),
        )
        .subcommand(
            Command::new("set")
                .about("Add a hardcoded KEY=VALUE to forwarded envvars")
                .arg(Arg::new("keyvalue").required(true).help("KEY=VALUE pair")),
        )
        .subcommand(
            Command::new("remove")
                .about("Remove a name or pattern from forwarded envvars")
                .arg(Arg::new("pattern").required(true).help("Var name or glob pattern to remove")),
        )
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .global(true)
                .help("Bulker configuration file"),
        )
}

pub fn dispatch(matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("add", sub_m)) => run_add(sub_m, matches),
        Some(("set", sub_m)) => run_set(sub_m, matches),
        Some(("remove", sub_m)) => run_remove(sub_m, matches),
        None => run_show(matches),
        _ => unreachable!(),
    }
}

fn run_show(matches: &ArgMatches) -> Result<()> {
    let (config, _) = load_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;

    // Show default allowlist
    let defaults: Vec<&str> = DEFAULT_ENVVARS.to_vec();
    println!("Default allowlist ({} patterns):", defaults.len());
    println!("  {}", defaults.join(", "));
    println!();

    // Show user additions from config
    if config.bulker.envvars.is_empty() {
        println!("User additions (from config): (none)");
    } else {
        println!("User additions (from config):");
        println!("  {}", config.bulker.envvars.join(", "));
    }
    println!();

    // Show resolved vars
    let mut patterns: Vec<String> = if config.bulker.no_default_envvars {
        Vec::new()
    } else {
        DEFAULT_ENVVARS.iter().map(|s| s.to_string()).collect()
    };
    crate::manifest::merge_lists(&mut patterns, &config.bulker.envvars);
    let resolved = expand_envvar_patterns(&patterns);
    println!("Resolved ({} vars forwarded from host):", resolved.len());
    if resolved.is_empty() {
        println!("  (none)");
    } else {
        // Print in rows of ~6
        for chunk in resolved.chunks(6) {
            let names: Vec<&str> = chunk.iter().map(|s| {
                s.split_once('=').map(|(k, _)| k).unwrap_or(s.as_str())
            }).collect();
            println!("  {}", names.join(", "));
        }
    }

    Ok(())
}

fn run_add(sub_m: &ArgMatches, parent_m: &ArgMatches) -> Result<()> {
    let (mut config, config_path) = load_config(parent_m.get_one::<String>("config").map(|s| s.as_str()))?;
    let config_path = config_path.context("No config file to write to. Run `bulker config init` first.")?;
    let pattern = sub_m.get_one::<String>("pattern").unwrap();

    if config.bulker.envvars.contains(pattern) {
        println!("'{}' already in envvars", pattern);
        return Ok(());
    }

    config.bulker.envvars.push(pattern.clone());
    config.write(&config_path)?;
    println!("Added '{}' to envvars", pattern);
    Ok(())
}

fn run_set(sub_m: &ArgMatches, parent_m: &ArgMatches) -> Result<()> {
    let (mut config, config_path) = load_config(parent_m.get_one::<String>("config").map(|s| s.as_str()))?;
    let config_path = config_path.context("No config file to write to. Run `bulker config init` first.")?;
    let keyvalue = sub_m.get_one::<String>("keyvalue").unwrap();

    if !keyvalue.contains('=') {
        anyhow::bail!("Expected KEY=VALUE format, got '{}'", keyvalue);
    }

    if config.bulker.envvars.contains(keyvalue) {
        println!("'{}' already in envvars", keyvalue);
        return Ok(());
    }

    config.bulker.envvars.push(keyvalue.clone());
    config.write(&config_path)?;
    println!("Added '{}' to envvars", keyvalue);
    Ok(())
}

fn run_remove(sub_m: &ArgMatches, parent_m: &ArgMatches) -> Result<()> {
    let (mut config, config_path) = load_config(parent_m.get_one::<String>("config").map(|s| s.as_str()))?;
    let config_path = config_path.context("No config file to write to. Run `bulker config init` first.")?;
    let pattern = sub_m.get_one::<String>("pattern").unwrap();

    if let Some(pos) = config.bulker.envvars.iter().position(|v| v == pattern) {
        config.bulker.envvars.remove(pos);
        config.write(&config_path)?;
        println!("Removed '{}' from envvars", pattern);
    } else {
        println!("'{}' not found in envvars", pattern);
    }

    Ok(())
}
