use anyhow::Result;
use clap::{ArgAction, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};

pub fn create_cli() -> Command {
    Command::new("list")
        .about("List installed crates")
        .after_help("\
EXAMPLES:
  bulkers crate list
  bulkers crate list --versions                 # show all installed versions
  bulkers crate list --simple                   # simple format for scripting")
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
                .help("Show all installed versions for each crate"),
        )
}

/// Find the longest common directory prefix across all crate paths.
/// Returns the common root as a String, or an empty string if none found.
fn find_common_root(paths: &[String]) -> String {
    if paths.is_empty() {
        return String::new();
    }

    // Convert each path to its parent directory components
    let component_lists: Vec<Vec<&str>> = paths
        .iter()
        .map(|p| {
            // Split on '/' and collect, then drop the last element (filename)
            let parts: Vec<&str> = p.split('/').collect();
            // Drop the last component to get the directory
            if parts.len() > 1 {
                parts[..parts.len() - 1].to_vec()
            } else {
                parts
            }
        })
        .collect();

    if component_lists.is_empty() {
        return String::new();
    }

    let first = &component_lists[0];
    let mut common_len = first.len();

    for components in &component_lists[1..] {
        let matching = first
            .iter()
            .zip(components.iter())
            .take_while(|(a, b)| a == b)
            .count();
        if matching < common_len {
            common_len = matching;
        }
    }

    if common_len == 0 {
        String::new()
    } else {
        first[..common_len].join("/")
    }
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
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let config = BulkerConfig::from_file(&config_path)?;

    let simple = matches.get_flag("simple");
    let show_versions = matches.get_flag("versions");
    let crates = config.crates();

    if crates.is_empty() {
        println!("No crates installed.");
        return Ok(());
    }

    // Simple mode: space-separated list of all namespace/crate:tag entries (unchanged)
    if simple {
        let mut entries = Vec::new();
        for (namespace, crate_map) in crates {
            for (crate_name, tag_map) in crate_map {
                for tag in tag_map.keys() {
                    entries.push(format!("{}/{}:{}", namespace, crate_name, tag));
                }
            }
        }
        println!("{}", entries.join(" "));
        return Ok(());
    }

    // Collect all paths for common root detection
    let all_paths: Vec<String> = crates
        .values()
        .flat_map(|crate_map| crate_map.values())
        .flat_map(|tag_map| tag_map.values())
        .map(|entry| entry.path.clone())
        .collect();

    let common_root = find_common_root(&all_paths);

    // Calculate column width: longest "namespace/crate_name" string
    let max_crate_width = crates
        .iter()
        .flat_map(|(namespace, crate_map)| {
            crate_map.keys().map(move |crate_name| {
                format!("{}/{}", namespace, crate_name).len()
            })
        })
        .max()
        .unwrap_or(20);

    // Print header
    if !common_root.is_empty() {
        println!("Crate root: {}", common_root);
    }
    println!();

    // Iterate BTreeMap (already sorted by namespace/crate_name)
    for (namespace, crate_map) in crates {
        for (crate_name, tag_map) in crate_map {
            let full_name = format!("{}/{}", namespace, crate_name);

            // Collect and sort tags descending
            let mut tags: Vec<String> = tag_map.keys().cloned().collect();
            sort_versions_desc(&mut tags);

            if show_versions {
                // Versions mode: show all tags, first on same line as crate name,
                // subsequent on continuation lines (indented, no crate name)
                let mut first = true;
                for tag in &tags {
                    if first {
                        println!("  {:<width$}  {}", full_name, tag, width = max_crate_width);
                        first = false;
                    } else {
                        // Indent to align with tag column
                        println!("  {:<width$}  {}", "", tag, width = max_crate_width);
                    }
                }
            } else {
                // Default mode: show only the latest (first after sort), with (+N more) hint
                let latest = tags.first().map(|s| s.as_str()).unwrap_or("default");
                let extra = tags.len().saturating_sub(1);
                if extra > 0 {
                    println!(
                        "  {:<width$}  {}  (+{} more)",
                        full_name,
                        latest,
                        extra,
                        width = max_crate_width
                    );
                } else {
                    println!("  {:<width$}  {}", full_name, latest, width = max_crate_width);
                }
            }
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
        // 1.0.14-dev first (highest minor version), then 1.0.13, 1.0.9, 1.0.4, then default last
        assert_eq!(tags.last().unwrap(), "default");
        assert_eq!(tags.first().unwrap(), "1.0.14-dev");
    }

    #[test]
    fn test_sort_versions_desc_only_default() {
        let mut tags = vec!["default".to_string()];
        sort_versions_desc(&mut tags);
        assert_eq!(tags, vec!["default"]);
    }

    #[test]
    fn test_find_common_root_basic() {
        let paths = vec![
            "/home/user/bulker_crates/bulker/alpine/default/alpine".to_string(),
            "/home/user/bulker_crates/databio/pepatac/1.0.14-dev/pepatac".to_string(),
        ];
        let root = find_common_root(&paths);
        assert!(root.starts_with("/home/user/bulker_crates"));
    }

    #[test]
    fn test_find_common_root_empty() {
        let root = find_common_root(&[]);
        assert_eq!(root, "");
    }

    #[test]
    fn test_find_common_root_single() {
        let paths = vec!["/home/user/crates/alpine/default/alpine".to_string()];
        let root = find_common_root(&paths);
        assert_eq!(root, "/home/user/crates/alpine/default");
    }
}
