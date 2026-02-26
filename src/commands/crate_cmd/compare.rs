use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config::load_config;
use crate::digest;
use crate::manifest::parse_registry_path;
use crate::manifest_cache;

pub fn create_cli() -> Command {
    Command::new("compare")
        .about("Compare two cached crates")
        .after_help("\
EXAMPLES:
  bulkers crate compare databio/peppro:1.0.13 databio/peppro:1.0.14
  bulkers crate compare databio/peppro:1.0.13 databio/peppro:1.0.14 --json")
        .arg(
            Arg::new("crate_a")
                .required(true)
                .help("First crate to compare"),
        )
        .arg(
            Arg::new("crate_b")
                .required(true)
                .help("Second crate to compare"),
        )
        .arg(
            Arg::new("json")
                .long("json")
                .action(ArgAction::SetTrue)
                .help("Output as JSON"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let (config, _config_path) = load_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let json_output = matches.get_flag("json");

    let path_a = matches.get_one::<String>("crate_a").unwrap();
    let path_b = matches.get_one::<String>("crate_b").unwrap();

    let cv_a = parse_registry_path(path_a, &config.bulker.default_namespace);
    let cv_b = parse_registry_path(path_b, &config.bulker.default_namespace);

    let manifest_a = manifest_cache::load_cached(&cv_a)?
        .ok_or_else(|| anyhow::anyhow!(
            "Crate '{}' is not cached.", cv_a.display_name()
        ))?;
    let manifest_b = manifest_cache::load_cached(&cv_b)?
        .ok_or_else(|| anyhow::anyhow!(
            "Crate '{}' is not cached.", cv_b.display_name()
        ))?;

    let cmp = digest::compare_manifests(&manifest_a, &manifest_b);

    if json_output {
        println!("{}", serde_json::to_string_pretty(&cmp.to_json())?);
        return Ok(());
    }

    // Human-readable output
    let eq_sym = if cmp.digest_a == cmp.digest_b { "=" } else { "\u{2260}" };
    println!(
        "crate-manifest-digest: {}  {}  {}",
        &cmp.digest_a, eq_sym, &cmp.digest_b
    );

    // Show crate-image-digest if both are available
    let img_a = manifest_cache::read_digest_sidecar(&cv_a, "crate-image-digest");
    let img_b = manifest_cache::read_digest_sidecar(&cv_b, "crate-image-digest");
    if let (Some(ia), Some(ib)) = (&img_a, &img_b) {
        let eq_sym = if ia == ib { "=" } else { "\u{2260}" };
        println!("crate-image-digest:    {}  {}  {}", ia, eq_sym, ib);
    }

    println!();

    let shared = cmp.a_count + cmp.b_count - cmp.a_only.len() - cmp.b_only.len();
    println!(
        "Commands ({} shared, {} A-only, {} B-only):",
        shared,
        cmp.a_only.len(),
        cmp.b_only.len()
    );

    if !cmp.a_only.is_empty() {
        println!("  A only: {}", cmp.a_only.join(", "));
    }
    if !cmp.b_only.is_empty() {
        println!("  B only: {}", cmp.b_only.join(", "));
    }
    if !cmp.image_diffs.is_empty() {
        println!("  Image differs:");
        for diff in &cmp.image_diffs {
            println!(
                "    {}: {} \u{2192} {}",
                diff.command, diff.a_image, diff.b_image
            );
        }
    }
    if cmp.a_and_b_count > 0 {
        println!("  Identical: {} commands", cmp.a_and_b_count);
    }
    if let Some(same) = cmp.same_order {
        if !same {
            println!("  Order: differs");
        }
    }

    Ok(())
}
