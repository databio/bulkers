// src/imports.rs
//
// Runtime import resolution for bulker crates. All import-related logic lives here.
// When activating or exec-ing a crate, this module resolves its imports recursively
// to build the full list of CrateVars, reading from the manifest cache.

use anyhow::Result;
use std::collections::HashSet;

use crate::config::BulkerConfig;
use crate::manifest::{CrateVars, parse_registry_path};

/// Resolve all CrateVars (including imports) for a list of crates.
/// Returns a flat list of all CrateVars in dependency order.
pub fn resolve_cratevars_with_imports(
    config: &BulkerConfig,
    cratelist: &[CrateVars],
) -> Result<Vec<CrateVars>> {
    let mut all_vars = Vec::new();
    let mut visited = HashSet::new();

    for cv in cratelist {
        resolve_crate_vars(config, cv, &mut all_vars, &mut visited)?;
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
) -> Result<()> {
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
        let import_cv = parse_registry_path(import_path, &config.bulker.default_namespace);
        resolve_crate_vars(config, &import_cv, vars, visited)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Manifest, ManifestInner, PackageCommand};

    /// Helper to build a minimal BulkerConfig for tests.
    fn make_test_config() -> BulkerConfig {
        BulkerConfig {
            bulker: crate::config::BulkerSettings {
                container_engine: "docker".to_string(),
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
                host_network: true,
                system_volumes: true,
                tool_args: None,
                shell_prompt: None,
                apptainer_image_folder: None,
                engine_path: None,
            },
        }
    }

    #[test]
    fn test_resolve_missing_crate_gives_clear_error() {
        let config = make_test_config();
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
        let old_val = std::env::var("XDG_CONFIG_HOME").ok();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", tmpdir.path()); }

        let config = make_test_config();
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
                    docker_command: None,
                    docker_args: None,
                    dockerargs: None,
                    apptainer_args: None,
                    apptainer_command: None,
                    volumes: vec![],
                    envvars: vec![],
                    no_user: false,
                    no_network: false,
                    no_default_volumes: false,
                    workdir: None,
                }],
                host_commands: vec![],
                imports: vec![],
            },
        };
        crate::manifest_cache::save_to_cache(&cv, &manifest).unwrap();

        let result = resolve_cratevars_with_imports(&config, &[cv]).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].crate_name, "demo");

        // Restore env
        match old_val {
            Some(v) => unsafe { std::env::set_var("XDG_CONFIG_HOME", v); },
            None => unsafe { std::env::remove_var("XDG_CONFIG_HOME"); },
        }
    }
}
