use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config::load_config;
use crate::digest;
use crate::manifest::{is_local_path, load_local_manifest, parse_registry_paths, CrateVars, Manifest};
use crate::manifest_cache;

pub fn create_cli() -> Command {
    Command::new("install")
        .about("Pre-cache a crate manifest (and optionally pull container images)")
        .after_help("\
EXAMPLES:
  bulker crate install bulker/demo
  bulker crate install databio/pepatac:1.0.13
  bulker crate install -b bulker/demo             # also pull container images
  bulker crate install ./manifest.yaml            # cache from local file

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
            Arg::new("build")
                .short('b')
                .long("build")
                .action(ArgAction::SetTrue)
                .help("Build/pull container images"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let (config, _config_path) = load_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;

    let cratefile = matches.get_one::<String>("cratefile").unwrap();
    let build = matches.get_flag("build");

    if is_local_path(cratefile) {
        // Local manifest file
        let (cv, manifest) = load_local_manifest(cratefile)?;
        manifest_cache::save_to_cache(&cv, &manifest)?;
        if build {
            manifest_cache::pull_images(&config, &manifest)?;
            attempt_image_digest(&cv, &manifest);
        }
        println!("Cached: {}", cv.display_name());
    } else {
        // Registry path(s)
        let cratelist = parse_registry_paths(cratefile, &config.bulker.default_namespace)?;
        for cv in &cratelist {
            let mut visited = std::collections::HashSet::new();
            manifest_cache::ensure_cached_with_imports(&config, cv, true, &mut visited, 0)?;  // always fetch fresh on explicit install
            if build {
                let manifest = manifest_cache::load_cached(cv)?.unwrap();
                manifest_cache::pull_images(&config, &manifest)?;
                attempt_image_digest(cv, &manifest);
            }
            println!("Cached: {}", cv.display_name());
        }
    }

    Ok(())
}

/// Best-effort: resolve OCI digests and store the crate-image-digest sidecar.
fn attempt_image_digest(cv: &CrateVars, manifest: &Manifest) {
    let oci_digests = digest::resolve_oci_digests(manifest);
    if let Some(result) = digest::crate_image_digest(manifest, &oci_digests) {
        let _ = manifest_cache::write_digest_sidecar(cv, "crate-image-digest", &result.digest);
        log::info!("Stored crate-image-digest: {}", result.digest);
    } else {
        log::debug!("Could not compute crate-image-digest (some images not resolved)");
    }
}
