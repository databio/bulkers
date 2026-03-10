use anyhow::Result;
use clap::{ArgAction, ArgMatches, Command};
use std::collections::BTreeMap;

use crate::manifest::{Manifest};
use crate::manifest_cache;

pub fn create_cli() -> Command {
    Command::new("list")
        .about("List cached crates")
        .after_help("\
EXAMPLES:
  bulker crate list
  bulker crate list --versions                 # show all cached versions
  bulker crate list --simple                   # simple format for scripting")
        .arg(
            clap::Arg::new("simple")
                .long("simple")
                .short('s')
                .action(ArgAction::SetTrue)
                .help("Simple output format (space-separated namespace/crate:tag, for scripting)"),
        )
        .arg(
            clap::Arg::new("versions")
                .long("versions")
                .action(ArgAction::SetTrue)
                .help("Show all cached versions for each crate"),
        )
}

/// Parse a version tag into comparable parts for semver-aware sorting.
/// Returns a sort key where "default" sorts last (highest), and semver-like
/// strings sort by numeric components descending (newest first).
fn version_sort_key(tag: &str) -> (u8, Vec<i64>, String) {
    if tag == "default" {
        // "default" always sorts last
        return (1, vec![], tag.to_string());
    }

    // Try to parse as semver-like: split on '.' and '-', compare numerically
    // Strip a leading 'v' if present
    let stripped = tag.strip_prefix('v').unwrap_or(tag);

    // Split on both '.' and '-'
    let parts: Vec<&str> = stripped.split(['.', '-']).collect();
    let numeric_parts: Vec<i64> = parts
        .iter()
        .map(|p| p.parse::<i64>().unwrap_or(-1))
        .collect();

    (0, numeric_parts, tag.to_string())
}

/// Sort version tags descending: newest semver first, "default" last.
fn sort_versions_desc(tags: &mut Vec<String>) {
    tags.sort_by(|a, b| {
        let ka = version_sort_key(a);
        let kb = version_sort_key(b);

        // Compare primary bucket first (0 = semver, 1 = default)
        match ka.0.cmp(&kb.0) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }

        // Compare numeric parts descending (newest first = reverse order)
        match ka.1.cmp(&kb.1).reverse() {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }

        // Fallback: string comparison
        ka.2.cmp(&kb.2)
    });
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let simple = matches.get_flag("simple");
    let show_versions = matches.get_flag("versions");

    let cached = manifest_cache::list_cached()?;

    if cached.is_empty() {
        println!("No cached crates.");
        return Ok(());
    }

    // Simple mode: space-separated list of all namespace/crate:tag entries
    if simple {
        let entries: Vec<String> = cached.iter()
            .map(|(cv, _)| cv.display_name())
            .collect();
        println!("{}", entries.join(" "));
        return Ok(());
    }

    // Group by namespace/crate_name -> Vec<(tag, version, digest)>
    let mut grouped: BTreeMap<String, Vec<(String, String, Option<String>)>> = BTreeMap::new();
    for (cv, manifest_path) in &cached {
        let key = format!("{}/{}", cv.namespace, cv.crate_name);
        let digest = manifest_cache::read_digest_sidecar(cv, "crate-manifest-digest");
        let version = std::fs::read_to_string(manifest_path)
            .ok()
            .and_then(|contents| serde_yml::from_str::<Manifest>(&contents).ok())
            .and_then(|m| m.manifest.version)
            .unwrap_or_default();
        grouped.entry(key).or_default().push((cv.tag.clone(), version, digest));
    }

    // Calculate column widths
    let max_crate_width = grouped.keys().map(|k| k.len()).max().unwrap_or(20);
    let tag_width = 10;
    let version_width = 10;
    let digest_width = 12;

    println!();
    println!(
        "  {:<cw$}  {:<tw$}  {:<vw$}  {:<dw$}",
        "Crate", "Tag", "Version", "Digest",
        cw = max_crate_width, tw = tag_width, vw = version_width, dw = digest_width
    );
    println!(
        "  {:<cw$}  {:<tw$}  {:<vw$}  {:<dw$}",
        "─".repeat(max_crate_width), "─".repeat(tag_width), "─".repeat(version_width), "─".repeat(digest_width),
        cw = max_crate_width, tw = tag_width, vw = version_width, dw = digest_width
    );

    for (full_name, entries) in grouped {
        // Sort by tag
        let mut tag_list: Vec<String> = entries.iter().map(|(t, _, _)| t.clone()).collect();
        sort_versions_desc(&mut tag_list);

        // Build maps from tag -> version/digest for quick lookup
        let info_map: std::collections::HashMap<&str, (&str, Option<&str>)> = entries
            .iter()
            .map(|(t, v, d)| (t.as_str(), (v.as_str(), d.as_deref())))
            .collect();

        if show_versions {
            let mut first = true;
            for tag in &tag_list {
                let (version, digest) = info_map.get(tag.as_str()).copied().unwrap_or(("", None));
                let digest_str = digest.map(|d| &d[..12]).unwrap_or("");
                let version_str = if version.is_empty() { "" } else { version };
                if first {
                    println!(
                        "  {:<cw$}  {:<tw$}  {:<vw$}  {}",
                        full_name, tag, version_str, digest_str,
                        cw = max_crate_width, tw = tag_width, vw = version_width
                    );
                    first = false;
                } else {
                    println!(
                        "  {:<cw$}  {:<tw$}  {:<vw$}  {}",
                        "", tag, version_str, digest_str,
                        cw = max_crate_width, tw = tag_width, vw = version_width
                    );
                }
            }
        } else {
            let latest = tag_list.first().map(|s| s.as_str()).unwrap_or("default");
            let (version, digest) = info_map.get(latest).copied().unwrap_or(("", None));
            let digest_str = digest.map(|d| &d[..12]).unwrap_or("");
            let version_str = if version.is_empty() { "" } else { version };
            let extra = tag_list.len().saturating_sub(1);
            let extra_str = if extra > 0 {
                format!("  (+{} more tag{})", extra, if extra == 1 { "" } else { "s" })
            } else {
                String::new()
            };
            println!(
                "  {:<cw$}  {:<tw$}  {:<vw$}  {}{}",
                full_name, latest, version_str, digest_str, extra_str,
                cw = max_crate_width, tw = tag_width, vw = version_width
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_versions_desc_semver() {
        let mut tags = vec![
            "1.0.4".to_string(),
            "default".to_string(),
            "1.0.14-dev".to_string(),
            "1.0.13".to_string(),
            "1.0.9".to_string(),
        ];
        sort_versions_desc(&mut tags);
        assert_eq!(tags.last().unwrap(), "default");
        assert_eq!(tags.first().unwrap(), "1.0.14-dev");
    }

    #[test]
    fn test_sort_versions_desc_only_default() {
        let mut tags = vec!["default".to_string()];
        sort_versions_desc(&mut tags);
        assert_eq!(tags, vec!["default"]);
    }
}
