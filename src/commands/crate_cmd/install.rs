use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};
use crate::crate_ops::{get_crate_path, load_crate, load_imports};
use crate::manifest::load_remote_manifest;

pub fn create_cli() -> Command {
    Command::new("install")
        .about("Install a crate from a cratefile")
        .after_help("\
EXAMPLES:
  bulkers crate install bulker/demo
  bulkers crate install databio/pepatac:1.0.13
  bulkers crate install -f bulker/demo             # overwrite existing
  bulkers crate install -b bulker/demo             # also pull container images
  bulkers crate install ./manifest.yaml            # install from local file

CRATEFILE FORMAT:
  namespace/crate:tag    Registry shorthand (e.g., databio/pepatac:1.0.13)
  crate                  Uses default namespace \"bulker\", tag \"default\"
  ./path/to/file.yaml    Local cratefile
  https://url/file.yaml  Remote cratefile")
        .arg(
            Arg::new("cratefile")
                .required(true)
                .help("Cratefile: registry shorthand, URL, or local file path"),
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
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let mut config = BulkerConfig::from_file(&config_path)?;

    let cratefile = matches.get_one::<String>("cratefile").unwrap();
    let build = matches.get_flag("build");
    let force = matches.get_flag("force");

    // Detect if the argument is a local file path or URL vs registry shorthand
    let (manifest_path, registry_path) = if cratefile.starts_with('.')
        || cratefile.starts_with('/')
        || cratefile.starts_with("http://")
        || cratefile.starts_with("https://")
    {
        (Some(cratefile.as_str()), cratefile.as_str())
    } else {
        (None, cratefile.as_str())
    };

    let (manifest, cratevars) = load_remote_manifest(&config, registry_path, manifest_path)?;

    // Determine crate path
    let crate_path = if let Some(custom_path) = matches.get_one::<String>("path") {
        std::path::PathBuf::from(custom_path)
    } else {
        get_crate_path(&config, &cratevars)
    };

    // Handle imports (always recurse)
    if !manifest.manifest.imports.is_empty() {
        load_imports(&manifest, &mut config, &config_path, build)?;
    }

    // Load the main crate
    load_crate(&manifest, &cratevars, &mut config, &crate_path, build, force)?;

    // Store import references in the crate entry
    if !manifest.manifest.imports.is_empty() {
        if let Some(entry) = config.get_crate_entry_mut(&cratevars) {
            entry.imports = manifest.manifest.imports.clone();
        }
    }

    // Write updated config
    config.write(&config_path)?;

    println!("Installed crate: {}", cratevars.display_name());
    Ok(())
}
