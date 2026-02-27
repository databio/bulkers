use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config::load_config;
use crate::manifest::{is_local_path, is_url, load_local_manifest, load_url_manifest, parse_registry_paths};

pub fn create_cli() -> Command {
    Command::new("activate")
        .about("Put crate commands on PATH")
        .after_help("\
EXAMPLES:
  bulker activate bulker/demo
  bulker activate databio/pepatac:1.0.13
  bulker activate bulker/demo,bulker/pi        # multiple crates
  bulker activate demo                          # uses default namespace
  bulker activate -s bulker/demo                # strict: only crate commands in PATH
  bulker activate --echo bulker/demo            # print exports instead of launching shell
  bulker activate ./my-pipeline.yaml            # activate from local manifest file

CRATE FORMAT:
  namespace/crate:tag    Full path (e.g., databio/pepatac:1.0.13)
  crate                  Uses default namespace \"bulker\", tag \"default\"
  crate1,crate2          Multiple crates
  ./path/to/file.yaml    Local manifest file
  https://url/file.yaml  Remote manifest")
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
            Arg::new("strict_env")
                .long("strict-env")
                .action(ArgAction::SetTrue)
                .help("Clean container environment: only pass envvars allowlist (config + manifest)"),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .action(ArgAction::SetTrue)
                .help("Re-fetch manifests from registry even if cached"),
        )
        .arg(
            Arg::new("name")
                .short('n')
                .long("name")
                .help("Override crate identity for local manifests (e.g., bulker/biobase:0.1.0)"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let (config, config_path) = load_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;

    let registry_paths = matches.get_one::<String>("crate_registry_paths").unwrap();
    let echo = matches.get_flag("echo");
    let strict = matches.get_flag("strict");
    let strict_env = matches.get_flag("strict_env");
    let hide_prompt = matches.get_flag("hide-prompt");
    let force = matches.get_flag("force");
    let name_override = matches.get_one::<String>("name").map(|s| s.as_str());

    // Detect URL, local file path, or registry path
    let cratelist = if is_url(registry_paths) {
        let (cv, manifest) = load_url_manifest(registry_paths, name_override, &config.bulker.default_namespace)?;
        crate::manifest_cache::save_to_cache(&cv, &manifest)?;
        vec![cv]
    } else if is_local_path(registry_paths) {
        let (cv, manifest) = load_local_manifest(registry_paths, name_override, &config.bulker.default_namespace)?;
        crate::manifest_cache::save_to_cache(&cv, &manifest)?;
        vec![cv]
    } else {
        parse_registry_paths(registry_paths, &config.bulker.default_namespace)?
    };

    crate::activate::activate(&config, config_path.as_deref(), &cratelist, echo, strict, strict_env, !hide_prompt, force)
}
