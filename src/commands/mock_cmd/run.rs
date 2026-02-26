use anyhow::{Context, Result};
use clap::{Arg, ArgAction, ArgMatches, Command};
use std::path::PathBuf;

use crate::config::load_config;
use crate::manifest::{load_remote_manifest, parse_registry_paths};
use crate::mock;

pub fn create_cli() -> Command {
    Command::new("run")
        .about("Replay pre-recorded container outputs (no containers needed)")
        .after_help("\
EXAMPLES:
  bulker mock run databio/pepatac:1.0.13 outputs.json
  bulker mock run bulker/demo outputs.json --echo
  bulker mock run -s bulker/demo outputs.json    # strict: only mock commands in PATH

The run subcommand loads a crate using pre-recorded outputs from an outputs.json
file instead of real containers. Use 'bulker mock record' to create the outputs.json.")
        .arg(
            Arg::new("crate_registry_paths")
                .required(true)
                .help("Crate(s) to mock (comma-separated for multiple)"),
        )
        .arg(
            Arg::new("outputs_json")
                .required(true)
                .help("Path to the outputs.json file with recorded command outputs"),
        )
        .arg(
            Arg::new("strict")
                .short('s')
                .long("strict")
                .action(ArgAction::SetTrue)
                .help("Strict mode: only mock commands available in PATH"),
        )
        .arg(
            Arg::new("echo")
                .long("echo")
                .action(ArgAction::SetTrue)
                .help("Echo export commands instead of launching shell"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let (config, _config_path) = load_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;

    let registry_paths = matches.get_one::<String>("crate_registry_paths").unwrap();
    let cratelist = parse_registry_paths(registry_paths, &config.bulker.default_namespace);
    let strict = matches.get_flag("strict");
    let echo = matches.get_flag("echo");

    let outputs_json = matches.get_one::<String>("outputs_json").unwrap();
    let outputs_path = PathBuf::from(outputs_json);
    let outputs_abs = if outputs_path.is_absolute() {
        outputs_path
    } else {
        std::env::current_dir()
            .context("Failed to get current directory")?
            .join(outputs_path)
    };

    if !outputs_abs.exists() {
        anyhow::bail!(
            "Outputs file not found: {}. Use 'bulker mock record' to create one.",
            outputs_abs.display()
        );
    }

    // Create a temp directory for the mock crate
    let mock_dir = tempfile::tempdir().context("Failed to create temp directory for mock crate")?;
    let mock_crate_path = mock_dir.path();

    // Load all manifests and create mock shims
    let mut all_mock_paths = Vec::new();
    for cv in &cratelist {
        let (manifest, _cratevars) = load_remote_manifest(&config, &cv.display_name(), None)?;
        let crate_subdir = mock_crate_path.join(format!(
            "{}_{}_{}",
            cv.namespace, cv.crate_name, cv.tag
        ));
        mock::load_mock_crate(&manifest, &crate_subdir)?;
        all_mock_paths.push(crate_subdir.to_string_lossy().to_string());
    }

    let mock_path_str = all_mock_paths.join(":");
    let newpath = if strict {
        mock_path_str.clone()
    } else {
        let current_path = std::env::var("PATH").unwrap_or_default();
        format!("{}:{}", mock_path_str, current_path)
    };

    let outputs_abs_str = outputs_abs.to_string_lossy().to_string();

    if echo {
        println!("export PATH=\"{}\"", newpath);
        println!("export BULKER_MOCK_OUTPUTS=\"{}\"", outputs_abs_str);
        println!("export BULKERCRATE=\"mock:{}\"", registry_paths);
        // Keep the temp dir alive by leaking it (user is responsible for cleanup)
        let _ = mock_dir.keep();
        return Ok(());
    }

    // Launch an interactive shell with mock environment
    // SAFETY: called in main thread before exec replaces the process
    unsafe {
        std::env::set_var("PATH", &newpath);
        std::env::set_var("BULKER_MOCK_OUTPUTS", &outputs_abs_str);
        std::env::set_var("BULKERCRATE", format!("mock:{}", registry_paths));
    }

    // Keep temp dir alive for the duration of the shell
    let _keep = mock_dir;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let status = std::process::Command::new(&shell)
        .status()
        .with_context(|| format!("Failed to launch shell: {}", shell))?;

    std::process::exit(status.code().unwrap_or(1));
}
