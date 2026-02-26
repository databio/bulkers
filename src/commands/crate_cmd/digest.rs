use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config::load_config;
use crate::digest;
use crate::manifest::parse_registry_path;
use crate::manifest_cache;

pub fn create_cli() -> Command {
    Command::new("digest")
        .about("Show content-addressable digest for a cached crate")
        .after_help("\
EXAMPLES:
  bulker crate digest databio/peppro:1.0.14
  bulker crate digest databio/peppro:1.0.14 --verbose
  bulker crate digest databio/peppro:1.0.14 --resolve")
        .arg(
            Arg::new("crate_registry_path")
                .required(true)
                .help("Crate to compute digest for"),
        )
        .arg(
            Arg::new("verbose")
                .long("verbose")
                .action(ArgAction::SetTrue)
                .help("Show both digests and per-command detail"),
        )
        .arg(
            Arg::new("resolve")
                .long("resolve")
                .action(ArgAction::SetTrue)
                .help("Resolve OCI image digests from registries (requires network)"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let (config, _config_path) = load_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;

    let registry_path = matches.get_one::<String>("crate_registry_path").unwrap();
    let verbose = matches.get_flag("verbose");
    let resolve = matches.get_flag("resolve");

    let cv = parse_registry_path(registry_path, &config.bulker.default_namespace);
    let manifest = manifest_cache::load_cached(&cv)?
        .ok_or_else(|| anyhow::anyhow!(
            "Crate '{}' is not cached. Run 'bulker crate install {}' first.",
            cv.display_name(), cv.display_name()
        ))?;

    let result = digest::crate_manifest_digest(&manifest);

    // Ensure sidecar is written
    manifest_cache::write_digest_sidecar(&cv, "crate-manifest-digest", &result.digest)?;

    if !verbose {
        println!("{}", result.digest);
        return Ok(());
    }

    // Verbose output
    println!("Crate: {}", cv.display_name());
    println!("crate-manifest-digest:  {}", result.digest);
    println!("  sorted:               {}", result.sorted_digest);

    // Show crate-image-digest
    let image_digest = manifest_cache::read_digest_sidecar(&cv, "crate-image-digest");
    if let Some(ref d) = image_digest {
        println!("crate-image-digest:     {}", d);
    } else if resolve {
        log::info!("Resolving OCI digests from registries...");
        let oci_digests = digest::resolve_oci_digests(&manifest);
        if let Some(img_result) = digest::crate_image_digest(&manifest, &oci_digests) {
            manifest_cache::write_digest_sidecar(&cv, "crate-image-digest", &img_result.digest)?;
            println!("crate-image-digest:     {}", img_result.digest);
        } else {
            println!("crate-image-digest:     not available (some images could not be resolved)");
        }
    } else {
        println!("crate-image-digest:     not available (use --resolve to compute)");
    }

    println!();
    println!("Per-command pair digests:");
    for (cmd, img, pd) in &result.pair_digests {
        println!("  {}  {} ({})", pd, cmd, img);
    }

    Ok(())
}
