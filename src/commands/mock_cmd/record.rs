use anyhow::{Context, Result};
use clap::{Arg, ArgMatches, Command};
use std::path::PathBuf;

use crate::config::load_config;
use crate::manifest::{load_remote_manifest, parse_registry_paths};
use crate::mock;
use crate::process;

pub fn create_cli() -> Command {
    Command::new("record")
        .about("Record container command outputs for mock replay")
        .after_help("\
EXAMPLES:
  bulker mock record databio/pepatac:1.0.13 outputs.json -- python pipeline.py sample1
  bulker mock record bulker/demo outputs.json -- cowsay hello

The record subcommand runs a pipeline command with recording shims that capture
stdout, stderr, return codes, and newly created files for each container command.
The recordings are appended to the specified outputs.json file for later use with
'bulker mock run'.")
        .arg(
            Arg::new("crate_registry_paths")
                .required(true)
                .help("Crate(s) to record (comma-separated for multiple)"),
        )
        .arg(
            Arg::new("outputs_json")
                .required(true)
                .help("Path to write/append recorded outputs"),
        )
        .arg(
            Arg::new("cmd")
                .required(true)
                .num_args(1..)
                .trailing_var_arg(true)
                .help("Command and arguments to run"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let (config, _config_path) = load_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;

    let registry_paths = matches.get_one::<String>("crate_registry_paths").unwrap();
    let cratelist = parse_registry_paths(registry_paths, &config.bulker.default_namespace)?;

    let outputs_json = matches.get_one::<String>("outputs_json").unwrap();
    let outputs_path = PathBuf::from(outputs_json);
    let outputs_abs = if outputs_path.is_absolute() {
        outputs_path
    } else {
        std::env::current_dir()
            .context("Failed to get current directory")?
            .join(outputs_path)
    };

    let cmd_args: Vec<&String> = matches.get_many::<String>("cmd").unwrap().collect();

    // Create a temp directory for the recording crate
    let record_dir =
        tempfile::tempdir().context("Failed to create temp directory for recording crate")?;
    let record_crate_path = record_dir.path();

    // Initialize outputs.json if it does not exist
    if !outputs_abs.exists() {
        if let Some(parent) = outputs_abs.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create parent dir for: {}", outputs_abs.display()))?;
        }
        std::fs::write(&outputs_abs, "{}\n")
            .with_context(|| format!("Failed to initialize outputs file: {}", outputs_abs.display()))?;
        log::info!("Initialized outputs file: {}", outputs_abs.display());
    }

    // Load all manifests and create recording shims
    let mut all_record_paths = Vec::new();
    for cv in &cratelist {
        let (manifest, _cratevars) = load_remote_manifest(&config, &cv.display_name(), None)?;
        let crate_subdir = record_crate_path.join(format!(
            "{}_{}_{}",
            cv.namespace, cv.crate_name, cv.tag
        ));
        mock::load_recording_crate(&manifest, &config, &crate_subdir)?;
        all_record_paths.push(crate_subdir.to_string_lossy().to_string());
    }

    let record_path_str = all_record_paths.join(":");
    let current_path = std::env::var("PATH").unwrap_or_default();
    let newpath = format!("{}:{}", record_path_str, current_path);

    // Quote arguments with shell-escape
    let quoted_args: Vec<String> = cmd_args
        .iter()
        .map(|a| shell_escape::escape(std::borrow::Cow::Borrowed(a.as_str())).to_string())
        .collect();

    let merged_command = format!(
        "export PATH=\"{}\"; export BULKER_MOCK_RECORD_FILE=\"{}\"; {}",
        newpath,
        outputs_abs.display(),
        quoted_args.join(" ")
    );

    let exit_code = process::spawn_shell_and_wait(&merged_command)?;

    log::info!(
        "Recording complete. Outputs written to: {}",
        outputs_abs.display()
    );

    std::process::exit(exit_code);
}
