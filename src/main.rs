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
        .subcommand(commands::activate::create_cli())
        .subcommand(commands::exec::create_cli())
        .subcommand(commands::crate_cmd::create_cli())
        .subcommand(commands::config_cmd::create_cli())
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
        Some(("activate", sub_m)) => commands::activate::run(sub_m),
        Some(("exec", sub_m)) => commands::exec::run(sub_m),
        Some(("crate", sub_m)) => commands::crate_cmd::dispatch(sub_m),
        Some(("config", sub_m)) => commands::config_cmd::dispatch(sub_m),
        _ => unreachable!("subcommand required"),
    }
}
