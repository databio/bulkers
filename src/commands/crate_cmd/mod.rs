pub mod install;
pub mod inspect;
pub mod list;
pub mod uninstall;
pub mod update;

use anyhow::Result;
use clap::{Arg, ArgMatches, Command};

pub fn create_cli() -> Command {
    Command::new("crate")
        .about("Manage crates")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .global(true)
                .help("Bulker configuration file"),
        )
        .subcommand(install::create_cli())
        .subcommand(uninstall::create_cli())
        .subcommand(update::create_cli())
        .subcommand(list::create_cli())
        .subcommand(inspect::create_cli())
}

pub fn dispatch(matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("install", sub_m)) => install::run(sub_m),
        Some(("uninstall", sub_m)) => uninstall::run(sub_m),
        Some(("update", sub_m)) => update::run(sub_m),
        Some(("list", sub_m)) => list::run(sub_m),
        Some(("inspect", sub_m)) => inspect::run(sub_m),
        _ => unreachable!(),
    }
}
