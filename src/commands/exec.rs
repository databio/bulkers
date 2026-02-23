use anyhow::{Context, Result};
use clap::{Arg, ArgAction, ArgMatches, Command};
use std::os::unix::process::CommandExt;
use std::sync::atomic::Ordering;

use crate::activate::get_new_path;
use crate::config::{BulkerConfig, select_config};
use crate::manifest::parse_registry_paths;
use crate::process;

pub fn create_cli() -> Command {
    Command::new("exec")
        .about("Run a command in a crate environment")
        .after_help("\
EXAMPLES:
  bulkers exec bulker/demo -- cowsay hello
  bulkers exec databio/pepatac:1.0.13 -- samtools --version
  bulkers exec -s bulker/demo -- cowsay hi    # strict: only crate commands in PATH

CRATE FORMAT:
  namespace/crate:tag    Full path (e.g., databio/pepatac:1.0.13)
  crate                  Uses default namespace \"bulker\", tag \"default\"
  crate1,crate2          Multiple crates")
        .arg(
            Arg::new("crate_registry_paths")
                .required(true)
                .help("Crate(s) to use (comma-separated for multiple)"),
        )
        .arg(
            Arg::new("cmd")
                .required(true)
                .num_args(1..)
                .trailing_var_arg(true)
                .help("Command and arguments to run"),
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
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let config = BulkerConfig::from_file(&config_path)?;

    let registry_paths = matches.get_one::<String>("crate_registry_paths").unwrap();
    let cratelist = parse_registry_paths(registry_paths, &config.bulker.default_namespace);
    let strict = matches.get_flag("strict");

    let cmd_args: Vec<&String> = matches.get_many::<String>("cmd").unwrap().collect();

    let newpath = get_new_path(&config, &cratelist, strict)?;

    // Quote arguments with shell-escape
    let quoted_args: Vec<String> = cmd_args
        .iter()
        .map(|a| shell_escape::escape(std::borrow::Cow::Borrowed(a.as_str())).to_string())
        .collect();

    let merged_command = format!(
        "export PATH=\"{}\"; {}",
        newpath,
        quoted_args.join(" ")
    );

    // Set up signal forwarding
    process::setup_signal_forwarding();

    // Spawn child in a new session
    let child = unsafe {
        std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(&merged_command)
            .pre_exec(|| {
                nix::unistd::setsid()
                    .map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
                Ok(())
            })
            .spawn()
            .context("Failed to spawn child process")?
    };

    let child_pid = child.id() as i32;
    process::CHILD_PID.store(child_pid, Ordering::SeqCst);

    // Wait for child
    let mut child = child;
    let status = child.wait().context("Failed to wait on child process")?;

    std::process::exit(status.code().unwrap_or(1));
}
