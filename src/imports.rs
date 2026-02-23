// src/imports.rs
//
// Runtime import resolution for bulker crates. All import-related logic lives here.
// When activating or exec-ing a crate, this module resolves its imports recursively
// to build the full PATH, instead of copying shims at install time.

use anyhow::Result;
use std::collections::HashSet;

use crate::config::{BulkerConfig, CrateEntry};
use crate::manifest::{CrateVars, parse_registry_path};

/// Look up a crate entry by its registry path components.
pub fn get_crate_entry<'a>(config: &'a BulkerConfig, cratevars: &CrateVars) -> Option<&'a CrateEntry> {
    config.get_crate_entry(cratevars)
}

/// Build a PATH string that includes the crate's own path plus all imported crate paths (recursively).
pub fn resolve_paths_with_imports(
    config: &BulkerConfig,
    cratelist: &[CrateVars],
    strict: bool,
) -> Result<String> {
    let mut all_paths = Vec::new();
    let mut visited = HashSet::new();

    for cv in cratelist {
        resolve_crate_paths(config, cv, &mut all_paths, &mut visited)?;
    }

    let crate_path_str = all_paths.join(":");

    if strict {
        Ok(crate_path_str)
    } else {
        let current_path = std::env::var("PATH").unwrap_or_default();
        Ok(format!("{}:{}", crate_path_str, current_path))
    }
}

/// Recursively resolve a crate's path and all its import paths (depth-first).
fn resolve_crate_paths(
    config: &BulkerConfig,
    cratevars: &CrateVars,
    paths: &mut Vec<String>,
    visited: &mut HashSet<String>,
) -> Result<()> {
    let key = cratevars.display_name();
    if visited.contains(&key) {
        return Ok(());
    }
    visited.insert(key.clone());

    let entry = get_crate_entry(config, cratevars)
        .ok_or_else(|| anyhow::anyhow!(
            "Crate '{}' is not installed. Run 'bulkers crate list' to see installed crates, or 'bulkers crate install' to add one.",
            key
        ))?;

    paths.push(entry.path.clone());

    for import_path in &entry.imports {
        let import_cv = parse_registry_path(import_path, &config.bulker.default_namespace);
        resolve_crate_paths(config, &import_cv, paths, visited)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Helper to build a minimal BulkerConfig for tests.
    fn make_test_config() -> BulkerConfig {
        BulkerConfig {
            bulker: crate::config::BulkerSettings {
                container_engine: "docker".to_string(),
                default_crate_folder: "/tmp/crates".to_string(),
                default_namespace: "bulker".to_string(),
                registry_url: "http://hub.bulker.io/".to_string(),
                shell_path: "/bin/bash".to_string(),
                shell_rc: "$HOME/.bashrc".to_string(),
                executable_template: "docker_executable.tera".to_string(),
                shell_template: "docker_shell.tera".to_string(),
                build_template: "docker_build.tera".to_string(),
                rcfile: "start.sh".to_string(),
                rcfile_strict: "start_strict.sh".to_string(),
                volumes: vec!["$HOME".to_string()],
                envvars: vec!["DISPLAY".to_string()],
                crates: None,
                tool_args: None,
                shell_prompt: None,
                apptainer_image_folder: None,
            },
        }
    }

    #[test]
    fn test_resolve_missing_crate_gives_clear_error() {
        let config = make_test_config();
        let cv = CrateVars {
            namespace: "bulker".to_string(),
            crate_name: "nonexistent".to_string(),
            tag: "default".to_string(),
        };
        let result = resolve_paths_with_imports(&config, &[cv], false);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not installed"));
        assert!(err.contains("bulkers crate list"));
    }

    #[test]
    fn test_resolve_single_crate_no_imports() {
        let mut config = make_test_config();
        let mut ns = BTreeMap::new();
        let mut crate_map = BTreeMap::new();
        crate_map.insert("default".to_string(), CrateEntry {
            path: "/tmp/crates/bulker/demo/default".to_string(),
            imports: Vec::new(),
        });
        ns.insert("demo".to_string(), crate_map);
        config.bulker.crates = Some(BTreeMap::from([("bulker".to_string(), ns)]));

        let cv = CrateVars {
            namespace: "bulker".to_string(),
            crate_name: "demo".to_string(),
            tag: "default".to_string(),
        };
        let result = resolve_paths_with_imports(&config, &[cv], true).unwrap();
        assert_eq!(result, "/tmp/crates/bulker/demo/default");
    }

    #[test]
    fn test_resolve_crate_with_imports() {
        let mut config = make_test_config();

        // Parent crate with one import
        let parent_entry = CrateEntry {
            path: "/tmp/crates/bulker/parent/default".to_string(),
            imports: vec!["bulker/child".to_string()],
        };
        let child_entry = CrateEntry {
            path: "/tmp/crates/bulker/child/default".to_string(),
            imports: Vec::new(),
        };

        let mut parent_tags = BTreeMap::new();
        parent_tags.insert("default".to_string(), parent_entry);
        let mut child_tags = BTreeMap::new();
        child_tags.insert("default".to_string(), child_entry);

        let mut ns = BTreeMap::new();
        ns.insert("parent".to_string(), parent_tags);
        ns.insert("child".to_string(), child_tags);

        config.bulker.crates = Some(BTreeMap::from([("bulker".to_string(), ns)]));

        let cv = CrateVars {
            namespace: "bulker".to_string(),
            crate_name: "parent".to_string(),
            tag: "default".to_string(),
        };
        let result = resolve_paths_with_imports(&config, &[cv], true).unwrap();
        assert_eq!(
            result,
            "/tmp/crates/bulker/parent/default:/tmp/crates/bulker/child/default"
        );
    }

    #[test]
    fn test_resolve_circular_imports_handled() {
        let mut config = make_test_config();

        // Two crates that import each other
        let a_entry = CrateEntry {
            path: "/tmp/crates/bulker/a/default".to_string(),
            imports: vec!["bulker/b".to_string()],
        };
        let b_entry = CrateEntry {
            path: "/tmp/crates/bulker/b/default".to_string(),
            imports: vec!["bulker/a".to_string()],
        };

        let mut a_tags = BTreeMap::new();
        a_tags.insert("default".to_string(), a_entry);
        let mut b_tags = BTreeMap::new();
        b_tags.insert("default".to_string(), b_entry);

        let mut ns = BTreeMap::new();
        ns.insert("a".to_string(), a_tags);
        ns.insert("b".to_string(), b_tags);

        config.bulker.crates = Some(BTreeMap::from([("bulker".to_string(), ns)]));

        let cv = CrateVars {
            namespace: "bulker".to_string(),
            crate_name: "a".to_string(),
            tag: "default".to_string(),
        };
        // Should not infinite-loop; visited set breaks the cycle
        let result = resolve_paths_with_imports(&config, &[cv], true).unwrap();
        assert_eq!(
            result,
            "/tmp/crates/bulker/a/default:/tmp/crates/bulker/b/default"
        );
    }

    #[test]
    fn test_resolve_non_strict_appends_existing_path() {
        let mut config = make_test_config();
        let mut ns = BTreeMap::new();
        let mut crate_map = BTreeMap::new();
        crate_map.insert("default".to_string(), CrateEntry {
            path: "/tmp/crates/bulker/demo/default".to_string(),
            imports: Vec::new(),
        });
        ns.insert("demo".to_string(), crate_map);
        config.bulker.crates = Some(BTreeMap::from([("bulker".to_string(), ns)]));

        let cv = CrateVars {
            namespace: "bulker".to_string(),
            crate_name: "demo".to_string(),
            tag: "default".to_string(),
        };
        let result = resolve_paths_with_imports(&config, &[cv], false).unwrap();
        assert!(result.starts_with("/tmp/crates/bulker/demo/default:"));
    }
}
