//! Standalone manifest cache. Stores and retrieves crate manifests in a
//! filesystem-based cache at ~/.config/bulker/manifests/<ns>/<name>/<tag>/manifest.yaml.
//! Decoupled from the config `crates` map — activate auto-fetches on demand.

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::config::BulkerConfig;
use crate::digest;
use crate::manifest::{CrateVars, Manifest, load_remote_manifest, parse_docker_image_path, parse_registry_path};
use crate::templates;

/// Get the base cache directory for manifests.
pub fn cache_base_dir() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"));
    config_dir.join("bulker").join("manifests")
}

/// Get the cache path for a specific crate's manifest.
pub fn manifest_path(cv: &CrateVars) -> PathBuf {
    cache_base_dir()
        .join(&cv.namespace)
        .join(&cv.crate_name)
        .join(&cv.tag)
        .join("manifest.yaml")
}

/// Get the path for a digest sidecar file next to the cached manifest.
fn digest_sidecar_path(cv: &CrateVars, filename: &str) -> PathBuf {
    cache_base_dir()
        .join(&cv.namespace)
        .join(&cv.crate_name)
        .join(&cv.tag)
        .join(filename)
}

/// Read a cached digest sidecar file. Returns None if not present.
pub fn read_digest_sidecar(cv: &CrateVars, filename: &str) -> Option<String> {
    let path = digest_sidecar_path(cv, filename);
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Write a digest sidecar file.
pub fn write_digest_sidecar(cv: &CrateVars, filename: &str, digest: &str) -> Result<()> {
    let path = digest_sidecar_path(cv, filename);
    std::fs::write(&path, digest)
        .with_context(|| format!("Failed to write digest sidecar: {}", path.display()))?;
    Ok(())
}

/// Ensure the crate-manifest-digest sidecar exists. Computes and saves it if missing.
pub fn ensure_crate_manifest_digest(cv: &CrateVars) -> Result<Option<String>> {
    if let Some(d) = read_digest_sidecar(cv, "crate-manifest-digest") {
        return Ok(Some(d));
    }
    // Load manifest and compute
    if let Some(manifest) = load_cached(cv)? {
        let result = digest::crate_manifest_digest(&manifest);
        write_digest_sidecar(cv, "crate-manifest-digest", &result.digest)?;
        Ok(Some(result.digest))
    } else {
        Ok(None)
    }
}

/// Load a manifest from the filesystem cache. Returns None if not cached.
pub fn load_cached(cv: &CrateVars) -> Result<Option<Manifest>> {
    let path = manifest_path(cv);
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read cached manifest: {}", path.display()))?;
    let manifest: Manifest = serde_yaml::from_str(&contents)
        .with_context(|| format!("Failed to parse cached manifest: {}", path.display()))?;
    Ok(Some(manifest))
}

/// Save a manifest to the filesystem cache.
pub fn save_to_cache(cv: &CrateVars, manifest: &Manifest) -> Result<()> {
    let path = manifest_path(cv);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create cache dir: {}", parent.display()))?;
    }
    let yaml = serde_yaml::to_string(manifest)
        .context("Failed to serialize manifest")?;
    std::fs::write(&path, &yaml)
        .with_context(|| format!("Failed to write manifest cache: {}", path.display()))?;

    // Compute and store crate-manifest-digest sidecar
    let result = digest::crate_manifest_digest(manifest);
    let sidecar = path.parent().unwrap().join("crate-manifest-digest");
    let _ = std::fs::write(&sidecar, &result.digest);

    Ok(())
}

/// Ensure a manifest is cached. Fetches from registry if not present.
/// If `force` is true, always re-fetch.
pub fn ensure_cached(config: &BulkerConfig, cv: &CrateVars, force: bool) -> Result<Manifest> {
    if !force {
        if let Some(manifest) = load_cached(cv)? {
            return Ok(manifest);
        }
    }
    log::info!("Fetching manifest: {}", cv.display_name());
    let (manifest, _) = load_remote_manifest(config, &cv.display_name(), None)?;
    save_to_cache(cv, &manifest)?;
    Ok(manifest)
}

/// Recursively ensure a manifest and all its imports are cached.
pub fn ensure_cached_with_imports(
    config: &BulkerConfig,
    cv: &CrateVars,
    force: bool,
) -> Result<Manifest> {
    let manifest = ensure_cached(config, cv, force)?;
    for import_path in &manifest.manifest.imports {
        let import_cv = parse_registry_path(import_path, &config.bulker.default_namespace);
        ensure_cached_with_imports(config, &import_cv, force)?;
    }
    Ok(manifest)
}

/// List all cached manifests by walking the cache directory tree.
/// Returns Vec<(CrateVars, PathBuf)> sorted by namespace/crate/tag.
pub fn list_cached() -> Result<Vec<(CrateVars, PathBuf)>> {
    let base = cache_base_dir();
    let mut results = Vec::new();
    if !base.exists() {
        return Ok(results);
    }
    // Walk: base/<namespace>/<crate_name>/<tag>/manifest.yaml
    for ns_entry in std::fs::read_dir(&base)? {
        let ns_entry = ns_entry?;
        if !ns_entry.file_type()?.is_dir() { continue; }
        let namespace = ns_entry.file_name().to_string_lossy().to_string();
        for crate_entry in std::fs::read_dir(ns_entry.path())? {
            let crate_entry = crate_entry?;
            if !crate_entry.file_type()?.is_dir() { continue; }
            let crate_name = crate_entry.file_name().to_string_lossy().to_string();
            for tag_entry in std::fs::read_dir(crate_entry.path())? {
                let tag_entry = tag_entry?;
                if !tag_entry.file_type()?.is_dir() { continue; }
                let tag = tag_entry.file_name().to_string_lossy().to_string();
                let manifest_file = tag_entry.path().join("manifest.yaml");
                if manifest_file.exists() {
                    results.push((
                        CrateVars { namespace: namespace.clone(), crate_name: crate_name.clone(), tag },
                        manifest_file,
                    ));
                }
            }
        }
    }
    results.sort_by(|a, b| a.0.display_name().cmp(&b.0.display_name()));
    Ok(results)
}

/// Remove a cached manifest. Cleans up empty parent directories.
pub fn remove_cached(cv: &CrateVars) -> Result<()> {
    let path = manifest_path(cv);
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    // Clean up empty parent dirs (tag -> crate_name -> namespace)
    for ancestor in &[
        path.parent(),
        path.parent().and_then(|p| p.parent()),
        path.parent().and_then(|p| p.parent()).and_then(|p| p.parent()),
    ] {
        if let Some(dir) = ancestor {
            let _ = std::fs::remove_dir(dir); // fails silently if not empty
        }
    }
    Ok(())
}

/// Pull container images for all commands in a manifest.
pub fn pull_images(config: &BulkerConfig, manifest: &Manifest) -> Result<()> {
    let is_apptainer = config.bulker.container_engine == "apptainer";
    let build_template = templates::get_build_template(config);

    for pkg in &manifest.manifest.commands {
        let extra_args = config.host_tool_specific_args(pkg, "docker_args");

        let build_content = if is_apptainer {
            let (img_ns, img_name, _img_tag) = parse_docker_image_path(&pkg.docker_image);
            let apptainer_image = format!("{}-{}.sif", img_ns, img_name);
            let apptainer_fullpath = config
                .bulker
                .apptainer_image_folder
                .as_deref()
                .map(|f| format!("{}/{}", f, apptainer_image))
                .unwrap_or_else(|| apptainer_image.clone());

            templates::render_template_apptainer(
                build_template,
                "build",
                config,
                pkg,
                &extra_args,
                &apptainer_image,
                &apptainer_fullpath,
            )?
        } else {
            templates::render_template(build_template, "build", config, pkg, &extra_args)?
        };

        log::info!("Building image for: {}", pkg.command);
        let status = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(&build_content)
            .status()
            .context("Failed to run build script")?;
        if !status.success() {
            log::warn!("Build script failed for: {}", pkg.command);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ManifestInner, Manifest};

    #[test]
    fn test_cache_base_dir_ends_with_manifests() {
        let base = cache_base_dir();
        assert!(base.to_string_lossy().ends_with("bulker/manifests"));
    }

    #[test]
    fn test_manifest_path_structure() {
        let cv = CrateVars {
            namespace: "databio".to_string(),
            crate_name: "pepatac".to_string(),
            tag: "1.0.13".to_string(),
        };
        let path = manifest_path(&cv);
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("databio/pepatac/1.0.13/manifest.yaml"));
    }

    #[test]
    fn test_save_and_load_cached_roundtrip() {
        // Use a temporary directory as cache base by setting XDG_CONFIG_HOME
        let tmpdir = tempfile::tempdir().unwrap();
        let old_val = std::env::var("XDG_CONFIG_HOME").ok();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", tmpdir.path()); }

        let cv = CrateVars {
            namespace: "test".to_string(),
            crate_name: "demo".to_string(),
            tag: "default".to_string(),
        };

        let manifest = Manifest {
            manifest: ManifestInner {
                name: Some("demo".to_string()),
                version: Some("1.0".to_string()),
                commands: vec![crate::manifest::PackageCommand {
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
                host_commands: vec!["ls".to_string()],
                imports: vec![],
            },
        };

        save_to_cache(&cv, &manifest).unwrap();
        let loaded = load_cached(&cv).unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.manifest.commands.len(), 1);
        assert_eq!(loaded.manifest.commands[0].command, "cowsay");
        assert_eq!(loaded.manifest.host_commands, vec!["ls"]);

        // Restore env
        match old_val {
            Some(v) => unsafe { std::env::set_var("XDG_CONFIG_HOME", v); },
            None => unsafe { std::env::remove_var("XDG_CONFIG_HOME"); },
        }
    }

    #[test]
    fn test_load_cached_returns_none_when_not_cached() {
        let cv = CrateVars {
            namespace: "nonexistent_ns_xyz".to_string(),
            crate_name: "nonexistent_crate".to_string(),
            tag: "nonexistent_tag".to_string(),
        };
        let result = load_cached(&cv).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_list_cached_empty_dir() {
        // list_cached should not panic on empty/missing dir
        let result = list_cached();
        assert!(result.is_ok());
    }

    #[test]
    fn test_remove_cached_nonexistent() {
        let cv = CrateVars {
            namespace: "nonexistent_ns_xyz".to_string(),
            crate_name: "nonexistent_crate".to_string(),
            tag: "nonexistent_tag".to_string(),
        };
        // Should not error on removing something that doesn't exist
        let result = remove_cached(&cv);
        assert!(result.is_ok());
    }
}
