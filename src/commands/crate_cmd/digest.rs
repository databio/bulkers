use std::collections::HashMap;

use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::{json, Value};

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
  bulker crate digest databio/peppro:1.0.14 --resolve
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
                .help("Resolve OCI image digests from registries (requires network)"),
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

    if !verbose && !json {
        println!("{}", result.digest);
        return Ok(());
    }

    // crate-image-digest: with --resolve, only the fresh result counts (None if
    // any image failed), so a cached value is never shown beside fresh OCI digests
    let mut oci_digests = HashMap::new();
    let image_digest = if resolve {
        log::info!("Resolving OCI digests from registries...");
        oci_digests = digest::resolve_oci_digests(&manifest);
        let img = digest::crate_image_digest(&manifest, &oci_digests).map(|r| r.digest);
        if let Some(d) = &img {
            manifest_cache::write_digest_sidecar(&cv, "crate-image-digest", d)?;
        }
        img
    } else {
        manifest_cache::read_digest_sidecar(&cv, "crate-image-digest")
    };

    if json {
        let commands: Vec<Value> = result.pair_digests.iter().map(|(cmd, img, pd)| json!({
            "command": cmd,
            "image": img,
            "pair_digest": pd,
            "oci_digest": oci_digests.get(img),
        })).collect();
        println!("{}", serde_json::to_string_pretty(&json!({
            "crate": cv.display_name(),
            "crate_manifest_digest": result.digest,
            "crate_image_digest": image_digest,
            "commands": commands,
            "imports": manifest.manifest.imports,
        }))?);
        return Ok(());
    }

    // Verbose output
    println!("Crate: {}", cv.display_name());
    println!("crate-manifest-digest:  {}", result.digest);
    println!("  sorted:               {}", result.sorted_digest);
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
