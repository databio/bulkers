// src/imports.rs
//
// Runtime import resolution for bulker crates. All import-related logic lives here.
// When activating or exec-ing a crate, this module resolves its imports recursively
// to build the full list of CrateVars, reading from the manifest cache.

use anyhow::Result;
use std::collections::HashSet;

use crate::config::BulkerConfig;
use crate::manifest::{CrateVars, parse_registry_path};
use crate::manifest_cache::MAX_IMPORT_DEPTH;

/// Resolve all CrateVars (including imports) for a list of crates.
/// Returns a flat list of all CrateVars in dependency order.
pub fn resolve_cratevars_with_imports(
    config: &BulkerConfig,
    cratelist: &[CrateVars],
) -> Result<Vec<CrateVars>> {
    let mut all_vars = Vec::new();
    let mut visited = HashSet::new();

    for cv in cratelist {
        resolve_crate_vars(config, cv, &mut all_vars, &mut visited, 0)?;
    }

    Ok(all_vars)
}

/// Recursively collect CrateVars for a crate and all its imports.
/// Reads import lists from cached manifests (not from config crates map).
fn resolve_crate_vars(
    config: &BulkerConfig,
    cratevars: &CrateVars,
    vars: &mut Vec<CrateVars>,
    visited: &mut HashSet<String>,
    depth: usize,
) -> Result<()> {
    if depth >= MAX_IMPORT_DEPTH {
        anyhow::bail!(
            "Import depth exceeded {} for crate '{}'. Check for excessively deep import chains.",
            MAX_IMPORT_DEPTH,
            cratevars.display_name(),
        );
    }

    let key = cratevars.display_name();
    if visited.contains(&key) {
        return Ok(());
    }
    visited.insert(key.clone());

    // Load imports from the cached manifest (not from config crates map)
    let manifest = crate::manifest_cache::load_cached(cratevars)?
        .ok_or_else(|| anyhow::anyhow!(
            "Crate '{}' is not cached. Run 'bulker activate' to fetch it.",
            key
        ))?;

    vars.push(CrateVars {
        namespace: cratevars.namespace.clone(),
        crate_name: cratevars.crate_name.clone(),
        tag: cratevars.tag.clone(),
    });

    for import_path in &manifest.manifest.imports {
        let import_cv = parse_registry_path(import_path, &config.bulker.default_namespace)?;
        resolve_crate_vars(config, &import_cv, vars, visited, depth + 1)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BulkerConfig;
    use crate::manifest::{Manifest, ManifestInner, PackageCommand};

    #[test]
    fn test_resolve_missing_crate_gives_clear_error() {
        let config = BulkerConfig::test_default();
        let cv = CrateVars {
            namespace: "bulker".to_string(),
            crate_name: "nonexistent_xyz_test".to_string(),
            tag: "default".to_string(),
        };
        let result = resolve_cratevars_with_imports(&config, &[cv]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not cached"));
    }

    #[test]
    fn test_resolve_single_crate_from_cache() {
        // Set up temp XDG dir for test isolation
        let tmpdir = tempfile::tempdir().unwrap();
        let _guard = crate::test_util::EnvGuard::set("XDG_CONFIG_HOME", tmpdir.path());

        let config = BulkerConfig::test_default();
        let cv = CrateVars {
            namespace: "test_imports".to_string(),
            crate_name: "demo".to_string(),
            tag: "default".to_string(),
        };

        // Cache a manifest
        let manifest = Manifest {
            manifest: ManifestInner {
                name: Some("demo".to_string()),
                version: None,
                commands: vec![PackageCommand {
                    command: "cowsay".to_string(),
                    docker_image: "nsheff/cowsay:latest".to_string(),
                    ..Default::default()
                }],
                host_commands: vec![],
                imports: vec![],
            },
        };
        crate::manifest_cache::save_to_cache(&cv, &manifest).unwrap();

        let result = resolve_cratevars_with_imports(&config, &[cv]).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].crate_name, "demo");


    }

    use crate::test_util::make_manifest_with_imports;

    #[test]
    fn test_resolve_cycle_detection() {
        // Set up isolated cache
        let tmpdir = tempfile::tempdir().unwrap();
        let _guard = crate::test_util::EnvGuard::set("XDG_CONFIG_HOME", tmpdir.path());

        let config = BulkerConfig::test_default();

        // Crate A imports B, crate B imports A -- a cycle
        let cv_a = CrateVars {
            namespace: "cycle_imports".to_string(),
            crate_name: "alpha".to_string(),
            tag: "default".to_string(),
        };
        let cv_b = CrateVars {
            namespace: "cycle_imports".to_string(),
            crate_name: "beta".to_string(),
            tag: "default".to_string(),
        };

        let manifest_a = make_manifest_with_imports("alpha", vec!["cycle_imports/beta:default".to_string()]);
        let manifest_b = make_manifest_with_imports("beta", vec!["cycle_imports/alpha:default".to_string()]);

        crate::manifest_cache::save_to_cache(&cv_a, &manifest_a).unwrap();
        crate::manifest_cache::save_to_cache(&cv_b, &manifest_b).unwrap();

        // Should complete without infinite recursion; cycle broken by visited set
        let result = resolve_cratevars_with_imports(&config, &[cv_a]);
        assert!(result.is_ok(), "Cycle detection failed: {:?}", result.err());
        let vars = result.unwrap();
        // Both crates should appear exactly once
        assert_eq!(vars.len(), 2);
        assert!(vars.iter().any(|v| v.crate_name == "alpha"));
        assert!(vars.iter().any(|v| v.crate_name == "beta"));


    }

    #[test]
    fn test_resolve_depth_limit() {
        // Set up isolated cache
        let tmpdir = tempfile::tempdir().unwrap();
        let _guard = crate::test_util::EnvGuard::set("XDG_CONFIG_HOME", tmpdir.path());

        let config = BulkerConfig::test_default();

        // Create a chain that exceeds MAX_IMPORT_DEPTH
        let depth = MAX_IMPORT_DEPTH + 1;
        for i in 0..depth {
            let imports = if i + 1 < depth {
                vec![format!("depth_imports/chain_{}:default", i + 1)]
            } else {
                vec![]
            };
            let cv = CrateVars {
                namespace: "depth_imports".to_string(),
                crate_name: format!("chain_{}", i),
                tag: "default".to_string(),
            };
            let manifest = make_manifest_with_imports(&format!("chain_{}", i), imports);
            crate::manifest_cache::save_to_cache(&cv, &manifest).unwrap();
        }

        let cv_start = CrateVars {
            namespace: "depth_imports".to_string(),
            crate_name: "chain_0".to_string(),
            tag: "default".to_string(),
        };

        let result = resolve_cratevars_with_imports(&config, &[cv_start]);
        assert!(result.is_err(), "Should have failed with depth limit error");
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Import depth exceeded"), "Error message should mention depth: {}", err_msg);


    }
}
