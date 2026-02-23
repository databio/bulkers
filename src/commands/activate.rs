use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};
use crate::manifest::{parse_registry_paths, CrateVars};

/// Detect if a crate argument is a local file path (as opposed to a registry path).
fn is_local_path(s: &str) -> bool {
    s.starts_with('.')
        || s.starts_with('/')
        || s.ends_with(".yaml")
        || s.ends_with(".yml")
}

/// Load a local manifest file and cache it, returning CrateVars for it.
fn load_local_manifest(path: &str) -> anyhow::Result<Vec<CrateVars>> {
    let file_path = std::path::Path::new(path);
    let contents = std::fs::read_to_string(file_path)
        .map_err(|e| anyhow::anyhow!("Failed to read local manifest '{}': {}", path, e))?;
    let manifest: crate::manifest::Manifest = serde_yaml::from_str(&contents)
        .map_err(|e| anyhow::anyhow!("Failed to parse local manifest '{}': {}", path, e))?;

    // Derive CrateVars from the filename
    let stem = file_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "local".to_string());
    let cv = CrateVars {
        namespace: "local".to_string(),
        crate_name: stem,
        tag: "default".to_string(),
    };

    crate::manifest_cache::save_to_cache(&cv, &manifest)?;
    Ok(vec![cv])
}

pub fn create_cli() -> Command {
    Command::new("activate")
        .about("Put crate commands on PATH")
        .after_help("\
EXAMPLES:
  bulkers activate bulker/demo
  bulkers activate databio/pepatac:1.0.13
  bulkers activate bulker/demo,bulker/pi        # multiple crates
  bulkers activate demo                          # uses default namespace
  bulkers activate -s bulker/demo                # strict: only crate commands in PATH
  bulkers activate --echo bulker/demo            # print exports instead of launching shell
  bulkers activate ./my-pipeline.yaml            # activate from local manifest file

CRATE FORMAT:
  namespace/crate:tag    Full path (e.g., databio/pepatac:1.0.13)
  crate                  Uses default namespace \"bulker\", tag \"default\"
  crate1,crate2          Multiple crates
  ./path/to/file.yaml    Local manifest file")
        .arg(
            Arg::new("crate_registry_paths")
                .required(true)
                .help("Crate(s) to activate (comma-separated for multiple, or a local .yaml file)"),
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
            Arg::new("echo")
                .long("echo")
                .action(ArgAction::SetTrue)
                .help("Echo export commands instead of launching shell"),
        )
        .arg(
            Arg::new("hide-prompt")
                .long("hide-prompt")
                .action(ArgAction::SetTrue)
                .help("Do not modify the shell prompt"),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .action(ArgAction::SetTrue)
                .help("Re-fetch manifests from registry even if cached"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let config = BulkerConfig::from_file(&config_path)?;

    let registry_paths = matches.get_one::<String>("crate_registry_paths").unwrap();
    let echo = matches.get_flag("echo");
    let strict = matches.get_flag("strict");
    let hide_prompt = matches.get_flag("hide-prompt");
    let force = matches.get_flag("force");

    // Detect local file path vs registry path
    let cratelist = if is_local_path(registry_paths) {
        load_local_manifest(registry_paths)?
    } else {
        parse_registry_paths(registry_paths, &config.bulker.default_namespace)
    };

    crate::activate::activate(&config, &config_path, &cratelist, echo, strict, !hide_prompt, force)
}
