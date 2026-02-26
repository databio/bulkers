pub mod clean;
pub mod compare;
pub mod digest;
pub mod install;
pub mod inspect;
pub mod list;

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
        .subcommand(list::create_cli())
        .subcommand(inspect::create_cli())
        .subcommand(clean::create_cli())
        .subcommand(digest::create_cli())
        .subcommand(compare::create_cli())
}

pub fn dispatch(matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("install", sub_m)) => install::run(sub_m),
        Some(("list", sub_m)) => list::run(sub_m),
        Some(("inspect", sub_m)) => inspect::run(sub_m),
        Some(("clean", sub_m)) => clean::run(sub_m),
        Some(("digest", sub_m)) => digest::run(sub_m),
        Some(("compare", sub_m)) => compare::run(sub_m),
        _ => unreachable!(),
    }
}
