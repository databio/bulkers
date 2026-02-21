use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};
use crate::crate_ops::{get_crate_path, load_crate, load_imports};
use crate::manifest::load_remote_manifest;

pub fn create_cli() -> Command {
    Command::new("load")
        .about("Load a crate from a manifest")
        .after_help("\
EXAMPLES:
  bulkers load bulker/demo
  bulkers load databio/pepatac:1.0.13
  bulkers load -f bulker/demo             # overwrite existing
  bulkers load -b bulker/demo             # also pull container images
  bulkers load -m manifest.yaml my/crate  # load from local manifest file")
        .arg(
            Arg::new("crate_registry_paths")
                .required(true)
                .help("Crate to load, e.g. bulker/demo or namespace/crate:tag"),
        )
        .arg(
            Arg::new("manifest")
                .short('m')
                .long("manifest")
                .help("Path or URL to manifest file"),
        )
        .arg(
            Arg::new("path")
                .short('p')
                .long("path")
                .help("Custom crate installation path"),
        )
        .arg(
            Arg::new("build")
                .short('b')
                .long("build")
                .action(ArgAction::SetTrue)
                .help("Build/pull container images"),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .action(ArgAction::SetTrue)
                .help("Overwrite existing crate"),
        )
        .arg(
            Arg::new("recurse")
                .short('r')
                .long("recurse")
                .action(ArgAction::SetTrue)
                .help("Recursively load imported manifests"),
        )
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .help("Bulker configuration file"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let mut config = BulkerConfig::from_file(&config_path)?;

    let registry_paths = matches.get_one::<String>("crate_registry_paths").unwrap();
    let manifest_path = matches.get_one::<String>("manifest").map(|s| s.as_str());
    let build = matches.get_flag("build");
    let force = matches.get_flag("force");
    let recurse = matches.get_flag("recurse");

    let (manifest, cratevars) = load_remote_manifest(&config, registry_paths, manifest_path)?;

    // Determine crate path
    let crate_path = if let Some(custom_path) = matches.get_one::<String>("path") {
        std::path::PathBuf::from(custom_path)
    } else {
        get_crate_path(&config, &cratevars)
    };

    // Handle imports first
    if !manifest.manifest.imports.is_empty() {
        load_imports(&manifest, &mut config, &config_path, build, recurse)?;
    }

    // Load the main crate
    load_crate(&manifest, &cratevars, &mut config, &crate_path, build, force)?;

    // Write updated config
    config.write(&config_path)?;

    println!("Loaded crate: {}", cratevars.display_name());
    Ok(())
}
