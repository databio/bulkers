pub mod add;
pub mod get;
pub mod init;
pub mod remove;
pub mod set;
pub mod show;

use anyhow::Result;
use clap::{Arg, ArgMatches, Command};

fn is_list_key(key: &str) -> bool {
    matches!(key, "envvars" | "volumes")
}

pub fn create_cli() -> Command {
    Command::new("config")
        .about("Manage bulkers configuration")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .global(true)
                .help("Bulker configuration file"),
        )
        .subcommand(init::create_cli())
        .subcommand(show::create_cli())
        .subcommand(get::create_cli())
        .subcommand(set::create_cli())
        .subcommand(add::create_cli())
        .subcommand(remove::create_cli())
}

pub fn dispatch(matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("init", sub_m)) => init::run(sub_m),
        Some(("show", sub_m)) => show::run(sub_m),
        Some(("get", sub_m)) => get::run(sub_m),
        Some(("set", sub_m)) => set::run(sub_m),
        Some(("add", sub_m)) => add::run(sub_m),
        Some(("remove", sub_m)) => remove::run(sub_m),
        _ => unreachable!(),
    }
}
