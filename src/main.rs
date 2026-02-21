#[cfg(not(unix))]
compile_error!("bulkers requires a Unix-like operating system (Linux, macOS)");

mod activate;
mod commands;
mod config;
mod crate_ops;
mod manifest;
mod process;
mod templates;

use anyhow::Result;
use clap::{Arg, ArgAction, Command};


pub mod consts {
    pub const VERSION: &str = env!("CARGO_PKG_VERSION");
    pub const BIN_NAME: &str = "bulkers";
}

fn build_parser() -> Command {
    Command::new(consts::BIN_NAME)
        .bin_name(consts::BIN_NAME)
        .version(consts::VERSION)
        .author("Databio")
        .about("Multi-container environment manager")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .action(ArgAction::SetTrue)
                .global(true)
                .help("Enable verbose/debug logging"),
        )
        .subcommand(commands::init::create_cli())
        .subcommand(commands::load::create_cli())
        .subcommand(commands::activate::create_cli())
        .subcommand(commands::run::create_cli())
        .subcommand(commands::list::create_cli())
        .subcommand(commands::inspect::create_cli())
        .subcommand(commands::unload::create_cli())
        .subcommand(commands::reload::create_cli())
        .subcommand(commands::envvars::create_cli())
}

fn main() -> Result<()> {
    let app = build_parser();
    let matches = app.get_matches();

    // Initialize logging
    // SAFETY: called before any threads are spawned, single-threaded context
    unsafe {
        if matches.get_flag("verbose") {
            std::env::set_var("RUST_LOG", "debug");
        } else if std::env::var("RUST_LOG").is_err() {
            std::env::set_var("RUST_LOG", "info");
        }
    }
    env_logger::init();

    match matches.subcommand() {
        Some(("init", sub_m)) => commands::init::run(sub_m),
        Some(("load", sub_m)) => commands::load::run(sub_m),
        Some(("activate", sub_m)) => commands::activate::run(sub_m),
        Some(("run", sub_m)) => commands::run::run(sub_m),
        Some(("list", sub_m)) => commands::list::run(sub_m),
        Some(("inspect", sub_m)) => commands::inspect::run(sub_m),
        Some(("unload", sub_m)) => commands::unload::run(sub_m),
        Some(("reload", sub_m)) => commands::reload::run(sub_m),
        Some(("envvars", sub_m)) => commands::envvars::run(sub_m),
        _ => unreachable!("subcommand required"),
    }
}
