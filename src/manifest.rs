use anyhow::{Context, Result, bail};
use serde::{Deserialize, Deserializer, Serialize};

use crate::config::BulkerConfig;

/// Deserialize a Vec that treats null as an empty Vec.
fn null_as_empty_vec<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<Vec<T>>::deserialize(deserializer).map(|v| v.unwrap_or_default())
}

/// Parsed registry path components.
#[derive(Debug, Clone)]
pub struct CrateVars {
    pub namespace: String,
    pub crate_name: String,
    pub tag: String,
}

impl CrateVars {
    /// Display as "namespace/crate_name:tag"
    pub fn display_name(&self) -> String {
        format!("{}/{}:{}", self.namespace, self.crate_name, self.tag)
    }
}

/// Manifest file structure (top-level).
#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub manifest: ManifestInner,
}

/// Inner manifest data.
#[derive(Debug, Serialize, Deserialize)]
pub struct ManifestInner {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub commands: Vec<PackageCommand>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub host_commands: Vec<String>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub imports: Vec<String>,
}

/// A single command entry in the manifest.
#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct PackageCommand {
    pub command: String,
    pub docker_image: String,
    #[serde(default)]
    pub docker_command: Option<String>,
    #[serde(default)]
    pub docker_args: Option<String>,
    #[serde(default)]
    pub dockerargs: Option<String>,
    #[serde(default)]
    pub apptainer_args: Option<String>,
    #[serde(default)]
    pub apptainer_command: Option<String>,
    #[serde(default)]
    pub volumes: Vec<String>,
    #[serde(default)]
    pub envvars: Vec<String>,
    #[serde(default)]
    pub no_user: bool,
    #[serde(default)]
    pub no_network: bool,
    #[serde(default)]
    pub no_default_volumes: bool,
    #[serde(default)]
    pub workdir: Option<String>,
}

impl PackageCommand {
    /// Merge docker_args from the command's `dockerargs` and `docker_args` fields,
    /// plus any extra args passed in (e.g., host-tool-specific or environment).
    pub(crate) fn merged_docker_args(&self, extra_args: &[&str]) -> String {
        let mut all = String::new();
        if let Some(ref da) = self.dockerargs {
            all.push_str(da);
        }
        if let Some(ref da) = self.docker_args {
            if !all.is_empty() { all.push(' '); }
            all.push_str(da);
        }
        for extra in extra_args {
            if !extra.is_empty() {
                if !all.is_empty() { all.push(' '); }
                all.push_str(extra);
            }
        }
        all
    }
}

/// Validate that a crate path component contains only safe characters.
/// Allowed: alphanumeric, hyphen, underscore, dot.
fn validate_crate_component(s: &str, label: &str) -> Result<()> {
    if s.is_empty() {
        bail!("Empty {} in crate path", label);
    }
    if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
        bail!(
            "Invalid {} '{}': only alphanumeric characters, hyphens, underscores, and dots are allowed",
            label, s
        );
    }
    Ok(())
}

/// Parse a single registry path string like "namespace/crate:tag".
///
/// Defaults: namespace = default_namespace (from config), tag = "default".
pub fn parse_registry_path(path: &str, default_namespace: &str) -> Result<CrateVars> {
    let path = path.trim();

    // Split on ':'  to separate name from tag
    let (name_part, tag) = if let Some(idx) = path.rfind(':') {
        (&path[..idx], path[idx + 1..].to_string())
    } else {
        (path, "default".to_string())
    };

    // Split on '/' to separate namespace from crate name
    let (namespace, crate_name) = if let Some(idx) = name_part.find('/') {
        (name_part[..idx].to_string(), name_part[idx + 1..].to_string())
    } else {
        (default_namespace.to_string(), name_part.to_string())
    };

    validate_crate_component(&namespace, "namespace")?;
    validate_crate_component(&crate_name, "crate name")?;
    validate_crate_component(&tag, "tag")?;

    Ok(CrateVars {
        namespace,
        crate_name,
        tag,
    })
}

/// Parse comma-separated registry paths.
pub fn parse_registry_paths(paths: &str, default_namespace: &str) -> Result<Vec<CrateVars>> {
    paths
        .split(',')
        .map(|p| parse_registry_path(p.trim(), default_namespace))
        .collect()
}

fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Build the URL for fetching a manifest from the registry.
fn build_manifest_url(config: &BulkerConfig, cratevars: &CrateVars, filepath: Option<&str>) -> String {
    if let Some(fp) = filepath {
        return fp.to_string();
    }

    let base_url = config.bulker.registry_url.trim_end_matches('/');
    if cratevars.tag == "default" {
        format!(
            "{}/{}/{}.yaml",
            base_url, cratevars.namespace, cratevars.crate_name
        )
    } else {
        format!(
            "{}/{}/{}_{}.yaml",
            base_url, cratevars.namespace, cratevars.crate_name, cratevars.tag
        )
    }
}

/// Load a manifest from a remote URL or local file path.
pub fn load_remote_manifest(
    config: &BulkerConfig,
    registry_path: &str,
    filepath: Option<&str>,
) -> Result<(Manifest, CrateVars)> {
    let cratevars = parse_registry_path(registry_path, &config.bulker.default_namespace)?;
    let url = build_manifest_url(config, &cratevars, filepath);

    log::debug!("Loading manifest from: {}", url);

    let contents = if is_url(&url) {
        let resp = ureq::get(&url)
            .call()
            .with_context(|| format!("Failed to fetch manifest: {}", url))?;
        resp.into_string()
            .with_context(|| format!("Failed to read response from: {}", url))?
    } else {
        std::fs::read_to_string(&url)
            .with_context(|| format!("Failed to read manifest file: {}", url))?
    };

    let manifest: Manifest = serde_yml::from_str(&contents)
        .with_context(|| format!("Failed to parse manifest YAML from: {}", url))?;

    Ok((manifest, cratevars))
}

/// Detect if a crate argument is a local file path (as opposed to a registry path).
pub(crate) fn is_local_path(s: &str) -> bool {
    s.starts_with('.')
        || s.starts_with('/')
        || s.ends_with(".yaml")
        || s.ends_with(".yml")
}

/// Load a local manifest file, returning the parsed Manifest and derived CrateVars.
pub(crate) fn load_local_manifest(path: &str) -> Result<(CrateVars, Manifest)> {
    let file_path = std::path::Path::new(path);
    let contents = std::fs::read_to_string(file_path)
        .map_err(|e| anyhow::anyhow!("Failed to read local manifest '{}': {}", path, e))?;
    let manifest: Manifest = serde_yml::from_str(&contents)
        .map_err(|e| anyhow::anyhow!("Failed to parse local manifest '{}': {}", path, e))?;

    let stem = file_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "local".to_string());
    let cv = CrateVars {
        namespace: "local".to_string(),
        crate_name: stem,
        tag: "default".to_string(),
    };

    Ok((cv, manifest))
}

/// Merge a secondary list into a primary list, appending items not already present.
/// Preserves order of the primary list, appends new items in secondary order.
pub(crate) fn merge_lists(primary: &mut Vec<String>, secondary: &[String]) {
    for item in secondary {
        if !primary.contains(item) {
            primary.push(item.clone());
        }
    }
}

/// Compute the apptainer SIF image filename and full path for a docker image.
/// Returns (image_filename, full_path) where full_path includes the image folder if configured.
pub(crate) fn apptainer_image_paths(docker_image: &str, image_folder: Option<&str>) -> (String, String) {
    let (img_ns, img_name, _img_tag) = parse_docker_image_path(docker_image);
    let image_filename = format!("{}-{}.sif", img_ns, img_name);
    let full_path = image_folder
        .map(|f| format!("{}/{}", f, image_filename))
        .unwrap_or_else(|| image_filename.clone());
    (image_filename, full_path)
}

/// Parse a docker image path into (namespace, image_name, tag) for apptainer.
pub fn parse_docker_image_path(docker_image: &str) -> (String, String, String) {
    // e.g. "quay.io/biocontainers/samtools:1.9--h91753b0_8"
    let (name_part, tag) = if let Some(idx) = docker_image.rfind(':') {
        (&docker_image[..idx], docker_image[idx + 1..].to_string())
    } else {
        (docker_image, "latest".to_string())
    };

    // Get the last path component as image name, rest as namespace
    if let Some(idx) = name_part.rfind('/') {
        let namespace = name_part[..idx].replace('/', "-").replace('.', "-");
        let image = name_part[idx + 1..].to_string();
        (namespace, image, tag)
    } else {
        ("docker".to_string(), name_part.to_string(), tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_registry_path_full() {
        let cv = parse_registry_path("myns/mycrate:v1.0", "bulker").unwrap();
        assert_eq!(cv.namespace, "myns");
        assert_eq!(cv.crate_name, "mycrate");
        assert_eq!(cv.tag, "v1.0");
    }

    #[test]
    fn test_parse_registry_path_no_namespace() {
        let cv = parse_registry_path("mycrate:v1.0", "bulker").unwrap();
        assert_eq!(cv.namespace, "bulker");
        assert_eq!(cv.crate_name, "mycrate");
        assert_eq!(cv.tag, "v1.0");
    }

    #[test]
    fn test_parse_registry_path_no_tag() {
        let cv = parse_registry_path("myns/mycrate", "bulker").unwrap();
        assert_eq!(cv.namespace, "myns");
        assert_eq!(cv.crate_name, "mycrate");
        assert_eq!(cv.tag, "default");
    }

    #[test]
    fn test_parse_registry_path_bare() {
        let cv = parse_registry_path("mycrate", "bulker").unwrap();
        assert_eq!(cv.namespace, "bulker");
        assert_eq!(cv.crate_name, "mycrate");
        assert_eq!(cv.tag, "default");
    }

    #[test]
    fn test_parse_registry_paths_comma() {
        let paths = parse_registry_paths("a/b:1,c/d:2", "bulker").unwrap();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].namespace, "a");
        assert_eq!(paths[0].crate_name, "b");
        assert_eq!(paths[0].tag, "1");
        assert_eq!(paths[1].namespace, "c");
        assert_eq!(paths[1].crate_name, "d");
        assert_eq!(paths[1].tag, "2");
    }

    #[test]
    fn test_parse_registry_path_rejects_shell_injection() {
        assert!(parse_registry_path("demo$(whoami)", "bulker").is_err());
    }

    #[test]
    fn test_parse_registry_path_rejects_semicolon() {
        assert!(parse_registry_path("demo;rm -rf /", "bulker").is_err());
    }

    #[test]
    fn test_parse_registry_path_rejects_backtick() {
        assert!(parse_registry_path("demo`whoami`", "bulker").is_err());
    }

    #[test]
    fn test_parse_registry_path_rejects_spaces() {
        assert!(parse_registry_path("demo crate", "bulker").is_err());
    }

    #[test]
    fn test_parse_registry_path_rejects_quotes() {
        assert!(parse_registry_path("demo\"crate", "bulker").is_err());
    }

    #[test]
    fn test_parse_registry_path_accepts_valid_names() {
        assert!(parse_registry_path("my-ns/my_crate.v2:1.0.0", "bulker").is_ok());
    }

    #[test]
    fn test_parse_registry_path_rejects_empty_crate_name() {
        assert!(parse_registry_path("ns/:tag", "bulker").is_err());
    }

    #[test]
    fn test_build_manifest_url_default_tag_omits_suffix() {
        let config = crate::config::BulkerConfig::test_with_registry("http://hub.bulker.io/");
        let cv = CrateVars {
            namespace: "bulker".to_string(),
            crate_name: "alpine".to_string(),
            tag: "default".to_string(),
        };
        let url = build_manifest_url(&config, &cv, None);
        assert_eq!(url, "http://hub.bulker.io/bulker/alpine.yaml");
    }

    #[test]
    fn test_build_manifest_url_versioned_tag_includes_suffix() {
        let config = crate::config::BulkerConfig::test_with_registry("http://hub.bulker.io/");
        let cv = CrateVars {
            namespace: "databio".to_string(),
            crate_name: "pepatac".to_string(),
            tag: "1.0.13".to_string(),
        };
        let url = build_manifest_url(&config, &cv, None);
        assert_eq!(url, "http://hub.bulker.io/databio/pepatac_1.0.13.yaml");
    }

    #[test]
    fn test_manifest_null_commands_parses_as_empty() {
        let yaml = r#"manifest:
  name: alpine
  commands: null
  host_commands:
  - ls
"#;
        let manifest: Manifest = serde_yml::from_str(yaml).unwrap();
        assert!(manifest.manifest.commands.is_empty());
        assert_eq!(manifest.manifest.host_commands, vec!["ls"]);
    }

    #[test]
    fn test_manifest_null_host_commands_parses_as_empty() {
        let yaml = r#"manifest:
  name: test
  commands:
  - command: cowsay
    docker_image: nsheff/cowsay
  host_commands: null
"#;
        let manifest: Manifest = serde_yml::from_str(yaml).unwrap();
        assert_eq!(manifest.manifest.commands.len(), 1);
        assert!(manifest.manifest.host_commands.is_empty());
    }

    #[test]
    fn test_manifest_null_imports_parses_as_empty() {
        let yaml = r#"manifest:
  name: test
  commands: []
  imports: null
"#;
        let manifest: Manifest = serde_yml::from_str(yaml).unwrap();
        assert!(manifest.manifest.imports.is_empty());
    }


    #[test]
    fn test_parse_docker_image_path() {
        let (ns, img, tag) = parse_docker_image_path("quay.io/biocontainers/samtools:1.9--h91753b0_8");
        assert_eq!(ns, "quay-io-biocontainers");
        assert_eq!(img, "samtools");
        assert_eq!(tag, "1.9--h91753b0_8");
    }

    #[test]
    fn test_parse_docker_image_simple() {
        let (ns, img, tag) = parse_docker_image_path("python:3.7.4");
        assert_eq!(ns, "docker");
        assert_eq!(img, "python");
        assert_eq!(tag, "3.7.4");
    }
}
