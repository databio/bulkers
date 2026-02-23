use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};
use crate::manifest::parse_registry_paths;

pub fn create_cli() -> Command {
    Command::new("activate")
        .about("Put crate commands on PATH")
        .after_help("\
EXAMPLES:
  bulkers activate bulker/demo
  bulkers activate databio/pepatac:1.0.13
  bulkers activate bulker/demo,bulker/pi        # multiple crates
  bulkers activate demo                          # uses default namespace
  bulkers activate -s bulker/demo                # strict: only crate commands in PATH
  bulkers activate --echo bulker/demo            # print exports instead of launching shell

CRATE FORMAT:
  namespace/crate:tag    Full path (e.g., databio/pepatac:1.0.13)
  crate                  Uses default namespace \"bulker\", tag \"default\"
  crate1,crate2          Multiple crates")
        .arg(
            Arg::new("crate_registry_paths")
                .required(true)
                .help("Crate(s) to activate (comma-separated for multiple)"),
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
                .long("echo")
                .action(ArgAction::SetTrue)
                .help("Echo export commands instead of launching shell"),
        )
        .arg(
            Arg::new("hide-prompt")
                .long("hide-prompt")
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
    let hide_prompt = matches.get_flag("hide-prompt");

    crate::activate::activate(&config, &config_path, &cratelist, echo, strict, !hide_prompt)
}
