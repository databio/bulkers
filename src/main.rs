#[cfg(not(unix))]
compile_error!("bulker requires a Unix-like operating system (Linux, macOS)");

mod activate;
mod commands;
mod config;
mod digest;
mod imports;
mod manifest;
mod manifest_cache;
mod mock;
mod process;
mod shimlink;
mod templates;

use anyhow::Result;
use clap::{Arg, ArgAction, Command};


pub mod consts {
    pub const VERSION: &str = env!("CARGO_PKG_VERSION");
    pub const BIN_NAME: &str = "bulker";
}

pub fn build_parser() -> Command {
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
        .subcommand(commands::init_shell::create_cli())
        .subcommand(commands::mock_cmd::create_cli())
        .subcommand(commands::completions::create_cli())
}

fn main() -> Result<()> {
    // Shimlink dispatch: if invoked as a symlink (argv[0] != "bulker"),
    // dispatch directly to the container command without clap parsing.
    if let Some(cmd_name) = shimlink::detect_shimlink_invocation() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        return shimlink::shimlink_exec(&cmd_name, &args);
    }

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
        Some(("init-shell", sub_m)) => commands::init_shell::run(sub_m),
        Some(("mock", sub_m)) => commands::mock_cmd::dispatch(sub_m),
        Some(("completions", sub_m)) => commands::completions::run(sub_m),
        _ => unreachable!("subcommand required"),
    }
}
