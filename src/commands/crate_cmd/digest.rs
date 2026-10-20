use std::collections::HashMap;

use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config::load_config;
use crate::digest;
use crate::manifest::{parse_registry_path, CrateVars, Manifest};
use crate::manifest_cache;

pub fn create_cli() -> Command {
    Command::new("digest")
        .about("Show content-addressable digest for a cached crate")
        .after_help("\
EXAMPLES:
  bulker crate digest databio/peppro:1.0.14
  bulker crate digest databio/peppro:1.0.14 --verbose
  bulker crate digest databio/peppro:1.0.14 --resolve
  bulker crate digest databio/peppro:1.0.14 --json
  bulker crate digest databio/peppro:1.0.14 --json --resolve")
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
            Arg::new("json")
                .long("json")
                .action(ArgAction::SetTrue)
                .help("Output as JSON"),
        )
        .arg(
            Arg::new("resolve")
                .long("resolve")
                .action(ArgAction::SetTrue)
                .help("Resolve OCI image digests from registries (requires network); applies to --verbose and --json"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let (config, _config_path) = load_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;

    let registry_path = matches.get_one::<String>("crate_registry_path").unwrap();
    let verbose = matches.get_flag("verbose");
    let resolve = matches.get_flag("resolve");
    let json = matches.get_flag("json");

    let cv = parse_registry_path(registry_path, &config.bulker.default_namespace)?;
    let manifest = manifest_cache::load_cached(&cv)?
        .ok_or_else(|| anyhow::anyhow!(
            "Crate '{}' is not cached. Run 'bulker crate install {}' first.",
            cv.display_name(), cv.display_name()
        ))?;

    let result = digest::crate_manifest_digest(&manifest);

    // Ensure sidecar is written
    manifest_cache::write_digest_sidecar(&cv, "crate-manifest-digest", &result.digest)?;

    if json {
        let (img, oci) = image_digests(&cv, &manifest, resolve)?;
        let report = digest::crate_digest_report(
            &cv.display_name(), &result, img, oci.as_ref(), &manifest.manifest.imports,
        );
        let out = serde_json::to_string_pretty(&report)?;
        println!("{}", out);
        return Ok(());
    }

    if !verbose {
        println!("{}", result.digest);
        return Ok(());
    }

    // Verbose output
    println!("Crate: {}", cv.display_name());
    println!("crate-manifest-digest:  {}", result.digest);
    println!("  sorted:               {}", result.sorted_digest);

    let (image_digest, _) = image_digests(&cv, &manifest, resolve)?;
    match image_digest {
        Some(d) => println!("crate-image-digest:     {}", d),
        None if resolve => println!("crate-image-digest:     not available (some images could not be resolved)"),
        None => println!("crate-image-digest:     not available (use --resolve to compute)"),
    }

    println!();
    println!("Per-command pair digests:");
    for (cmd, img, pd) in &result.pair_digests {
        println!("  {}  {} ({})", pd, cmd, img);
    }

    Ok(())
}

type ImageDigests = (Option<String>, Option<HashMap<String, String>>);

/// Returns (crate_image_digest, oci_digests_if_resolved).
/// Without `resolve`, the cached sidecar is used. With `resolve`, only the
/// freshly computed value is used (None if any image failed to resolve), so a
/// stale sidecar is never reported next to fresh per-command OCI digests.
fn image_digests(
    cv: &CrateVars,
    manifest: &Manifest,
    resolve: bool,
) -> Result<ImageDigests> {
    if !resolve {
        return Ok((manifest_cache::read_digest_sidecar(cv, "crate-image-digest"), None));
    }
    log::info!("Resolving OCI digests from registries...");
    let oci = digest::resolve_oci_digests(manifest);
    let img = match digest::crate_image_digest(manifest, &oci) {
        Some(r) => {
            manifest_cache::write_digest_sidecar(cv, "crate-image-digest", &r.digest)?;
            Some(r.digest)
        }
        None => None,
    };
    Ok((img, Some(oci)))
}
