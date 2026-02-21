use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};
use crate::manifest::parse_registry_paths;

pub fn create_cli() -> Command {
    Command::new("activate")
        .about("Start a new shell with crate commands in PATH")
        .after_help("\
EXAMPLES:
  bulkers activate bulker/demo
  bulkers activate databio/pepatac:1.0.13
  bulkers activate bulker/demo,bulker/pi    # multiple crates
  bulkers activate demo                     # uses default namespace
  bulkers activate -s bulker/demo           # strict: only crate commands in PATH
  bulkers activate -e bulker/demo           # print exports instead of launching shell")
        .arg(
            Arg::new("crate_registry_paths")
                .required(true)
                .help("Crate to activate, e.g. bulker/demo or namespace/crate:tag"),
        )
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .help("Bulker configuration file"),
        )
        .arg(
            Arg::new("strict")
                .short('s')
                .long("strict")
                .action(ArgAction::SetTrue)
                .help("Strict mode: only crate commands available in PATH"),
        )
        .arg(
            Arg::new("echo")
                .short('e')
                .long("echo")
                .action(ArgAction::SetTrue)
                .help("Echo export commands instead of launching shell"),
        )
        .arg(
            Arg::new("no-prompt")
                .short('p')
                .long("no-prompt")
                .action(ArgAction::SetTrue)
                .help("Do not modify the shell prompt"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let config = BulkerConfig::from_file(&config_path)?;

    let registry_paths = matches.get_one::<String>("crate_registry_paths").unwrap();
    let cratelist = parse_registry_paths(registry_paths, &config.bulker.default_namespace);

    let echo = matches.get_flag("echo");
    let strict = matches.get_flag("strict");
    let prompt = !matches.get_flag("no-prompt");

    crate::activate::activate(&config, &config_path, &cratelist, echo, strict, prompt)
}
