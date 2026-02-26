use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::activate::get_new_path;
use crate::config::load_config;
use crate::manifest::parse_registry_paths;
use crate::process;

pub fn create_cli() -> Command {
    Command::new("exec")
        .about("Run a command in a crate environment")
        .after_help("\
EXAMPLES:
  bulker exec bulker/demo -- cowsay hello
  bulker exec databio/pepatac:1.0.13 -- samtools --version
  bulker exec -s bulker/demo -- cowsay hi    # strict: only crate commands in PATH

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
        .arg(
            Arg::new("print_command")
                .short('p')
                .long("print-command")
                .action(ArgAction::SetTrue)
                .help("Print the generated docker/apptainer command instead of running it"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let (config, config_path) = load_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;

    let registry_paths = matches.get_one::<String>("crate_registry_paths").unwrap();
    let cratelist = parse_registry_paths(registry_paths, &config.bulker.default_namespace)?;
    let strict = matches.get_flag("strict");

    let cmd_args: Vec<&String> = matches.get_many::<String>("cmd").unwrap().collect();

    if matches.get_flag("print_command") {
        // SAFETY: called before any threads are spawned
        unsafe { std::env::set_var("BULKER_PRINT_COMMAND", "1"); }
    }

    let result = get_new_path(&config, &cratelist, strict, false)?;

    // Quote arguments with shell-escape
    let quoted_args: Vec<String> = cmd_args
        .iter()
        .map(|a| shell_escape::escape(std::borrow::Cow::Borrowed(a.as_str())).to_string())
        .collect();

    let crate_id = cratelist.first()
        .map(|cv| cv.display_name())
        .unwrap_or_default();

    let bulkercfg_export = match &config_path {
        Some(p) => format!("export BULKERCFG=\"{}\"; ", p.display()),
        None => String::new(),
    };
    let merged_command = format!(
        "export PATH=\"{}\"; export BULKERCRATE=\"{}\"; {}{}",
        result.path,
        crate_id,
        bulkercfg_export,
        quoted_args.join(" ")
    );

    let exit_code = process::spawn_shell_and_wait(&merged_command)?;

    // Clean up the ephemeral shimdir
    let _ = std::fs::remove_dir_all(&result.shimdir);

    std::process::exit(exit_code);
}
