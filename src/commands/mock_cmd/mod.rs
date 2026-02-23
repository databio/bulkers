pub mod record;
pub mod run;

use anyhow::Result;
use clap::{Arg, ArgMatches, Command};

pub fn create_cli() -> Command {
    Command::new("mock")
        .about("Mock container commands for CI testing")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .global(true)
                .help("Bulker configuration file"),
        )
        .subcommand(run::create_cli())
        .subcommand(record::create_cli())
}

pub fn dispatch(matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("run", sub_m)) => run::run(sub_m),
        Some(("record", sub_m)) => record::run(sub_m),
        _ => unreachable!(),
    }
}
