use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::manifest::PackageCommand;
use crate::manifest::parse_docker_image_path;
use crate::templates;

const BULKERCFG_ENV: &str = "BULKERCFG";

#[derive(Debug, Serialize, Deserialize)]
pub struct BulkerConfig {
    #[serde(default)]
    pub bulker: BulkerSettings,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BulkerSettings {
    #[serde(default = "default_container_engine")]
    pub container_engine: String,
    #[serde(default = "default_namespace")]
    pub default_namespace: String,
    #[serde(default = "default_registry_url")]
    pub registry_url: String,
    #[serde(default = "default_shell_path")]
    pub shell_path: String,
    #[serde(default = "default_shell_rc")]
    pub shell_rc: String,
    #[serde(default = "default_rcfile")]
    pub rcfile: String,
    #[serde(default = "default_rcfile_strict")]
    pub rcfile_strict: String,
    #[serde(default = "default_volumes")]
    pub volumes: Vec<String>,
    #[serde(default = "default_envvars")]
    pub envvars: Vec<String>,
    #[serde(default = "default_host_network")]
    pub host_network: bool,
    #[serde(default = "default_system_volumes")]
    pub system_volumes: bool,
    #[serde(default)]
    pub tool_args: Option<serde_yml::Value>,
    #[serde(default)]
    pub shell_prompt: Option<String>,
    #[serde(default)]
    pub apptainer_image_folder: Option<String>,
    #[serde(default)]
    pub engine_path: Option<String>,
}

fn default_container_engine() -> String {
    detect_engine().unwrap_or_else(|| "docker".to_string())
}

fn default_namespace() -> String {
    "bulker".to_string()
}

fn default_registry_url() -> String {
    "http://hub.bulker.io/".to_string()
}

fn default_shell_path() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
}

fn default_shell_rc() -> String {
    "$HOME/.bashrc".to_string()
}

fn default_rcfile() -> String {
    "start.sh".to_string()
}

fn default_rcfile_strict() -> String {
    "start_strict.sh".to_string()
}

fn default_host_network() -> bool {
    !cfg!(target_os = "macos") // true on Linux, false on macOS
}

fn default_system_volumes() -> bool {
    !cfg!(target_os = "macos") // true on Linux, false on macOS
}

fn default_volumes() -> Vec<String> {
    vec!["$HOME".to_string()]
}

fn default_envvars() -> Vec<String> {
    vec!["DISPLAY".to_string()]
}

impl BulkerSettings {
    /// Fix serde_yml's behavior of deserializing YAML null as the string "null".
    fn sanitize(&mut self) {
        if self.container_engine == "null" || self.container_engine.is_empty() {
            self.container_engine = default_container_engine();
        }
        if self.engine_path.as_deref() == Some("null") || self.engine_path.as_deref() == Some("") {
            self.engine_path = None;
        }
        if self.shell_prompt.as_deref() == Some("null") {
            self.shell_prompt = None;
        }
        if self.apptainer_image_folder.as_deref() == Some("null") {
            self.apptainer_image_folder = None;
        }
    }
}

impl BulkerConfig {
    pub fn from_file(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;
        let mut config: BulkerConfig = serde_yml::from_str(&contents)
            .with_context(|| format!("Failed to parse config: {}", path.display()))?;

        // Fix YAML null values deserialized as the string "null"
        config.bulker.sanitize();

        // Expand env vars in key paths
        config.bulker.shell_path = expand_path(&config.bulker.shell_path);
        config.bulker.shell_rc = expand_path(&config.bulker.shell_rc);
        if let Some(ref folder) = config.bulker.apptainer_image_folder {
            config.bulker.apptainer_image_folder = Some(expand_path(folder));
        }
        if let Some(ref ep) = config.bulker.engine_path {
            config.bulker.engine_path = Some(expand_path(ep));
        }

        Ok(config)
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        let yaml = serde_yml::to_string(self)
            .context("Failed to serialize config")?;
        std::fs::write(path, &yaml)
            .with_context(|| format!("Failed to write config: {}", path.display()))?;
        Ok(())
    }

    /// Get the resolved engine path. Returns the absolute path if set,
    /// otherwise falls back to the engine name string (current behavior).
    pub fn engine_path(&self) -> &str {
        self.bulker.engine_path.as_deref()
            .unwrap_or(&self.bulker.container_engine)
    }

    /// Look up host-tool-specific arguments from the config's tool_args.
    pub fn host_tool_specific_args(&self, pkg: &PackageCommand, arg_key: &str) -> String {
        let tool_args = match &self.bulker.tool_args {
            Some(v) => v,
            None => return String::new(),
        };

        let (img_ns, img_name, img_tag) = parse_docker_image_path(&pkg.docker_image);

        for tag in &[img_tag.as_str(), "default"] {
            if let Some(val) = tool_args
                .get(&img_ns)
                .and_then(|ns| ns.get(&img_name))
                .and_then(|img| img.get(*tag))
                .and_then(|t| t.get(arg_key))
                .and_then(|v| v.as_str())
            {
                return val.to_string();
            }
        }

        String::new()
    }
}

#[cfg(test)]
impl BulkerConfig {
    /// Build a minimal BulkerConfig for tests. Avoids auto-detection of
    /// container engine so tests are deterministic.
    pub fn test_default() -> Self {
        BulkerConfig {
            bulker: BulkerSettings {
                container_engine: "docker".to_string(),
                default_namespace: "bulker".to_string(),
                registry_url: "http://hub.bulker.io/".to_string(),
                shell_path: "/bin/bash".to_string(),
                shell_rc: "$HOME/.bashrc".to_string(),
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

    /// Like `test_default()` but with a custom registry URL.
    pub fn test_with_registry(registry_url: &str) -> Self {
        let mut config = Self::test_default();
        config.bulker.registry_url = registry_url.to_string();
        config
    }
}

impl Default for BulkerSettings {
    fn default() -> Self {
        let engine = default_container_engine();
        BulkerSettings {
            container_engine: engine.clone(),
            default_namespace: default_namespace(),
            registry_url: default_registry_url(),
            shell_path: default_shell_path(),
            shell_rc: default_shell_rc(),
            rcfile: default_rcfile(),
            rcfile_strict: default_rcfile_strict(),
            volumes: default_volumes(),
            envvars: default_envvars(),
            host_network: default_host_network(),
            system_volumes: default_system_volumes(),
            tool_args: None,
            shell_prompt: None,
            apptainer_image_folder: None,
            engine_path: resolve_engine_path(&engine),
        }
    }
}

impl Default for BulkerConfig {
    fn default() -> Self {
        BulkerConfig {
            bulker: BulkerSettings::default(),
        }
    }
}

// ─── engine detection ────────────────────────────────────────────────────────

/// Auto-detect available container engine. Returns Some("docker") or Some("apptainer"),
/// or None if neither is found.
pub fn detect_engine() -> Option<String> {
    if is_in_path("docker") {
        Some("docker".to_string())
    } else if is_in_path("apptainer") {
        Some("apptainer".to_string())
    } else {
        None
    }
}

/// Resolve the absolute path of a command using `which`.
/// Returns Some(path) if found, None otherwise.
pub fn resolve_engine_path(engine: &str) -> Option<String> {
    std::process::Command::new("which")
        .arg(engine)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn is_in_path(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ─── config loading ──────────────────────────────────────────────────────────

/// Load config: explicit arg > $BULKERCFG > default path > built-in defaults with cache attempt.
/// Returns (config, Option<config_path>). The path is None only when no file exists and caching failed.
pub fn load_config(arg: Option<&str>) -> Result<(BulkerConfig, Option<PathBuf>)> {
    // Step 1: explicit arg
    if let Some(path) = arg {
        let p = PathBuf::from(expand_path(path));
        if p.exists() {
            let config = BulkerConfig::from_file(&p)?;
            return Ok((config, Some(p)));
        }
        bail!("Config file not found: {}", p.display());
    }

    // Step 2: $BULKERCFG env var
    if let Ok(env_path) = std::env::var(BULKERCFG_ENV) {
        let p = PathBuf::from(expand_path(&env_path));
        if p.exists() {
            let config = BulkerConfig::from_file(&p)?;
            return Ok((config, Some(p)));
        }
        bail!("Config from ${} not found: {}", BULKERCFG_ENV, p.display());
    }

    // Step 3: default location
    let default_path = default_config_path();
    if default_path.exists() {
        let config = BulkerConfig::from_file(&default_path)?;
        return Ok((config, Some(default_path)));
    }

    // Step 4: no file found — build defaults, try to cache
    let config = BulkerConfig::default();
    match cache_config_to_disk(&config, &default_path) {
        Ok(()) => {
            eprintln!("Created config file: {}", default_path.display());
            Ok((config, Some(default_path)))
        }
        Err(_) => {
            // Read-only filesystem, permissions, etc. — continue without file
            Ok((config, None))
        }
    }
}

/// Write a config and its templates to disk as a cache.
pub fn cache_config_to_disk(config: &BulkerConfig, config_path: &Path) -> Result<()> {
    // Create config directory
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
    }

    // Write templates
    let templates_dir = config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("templates");
    templates::write_templates_to_dir(&templates_dir)?;

    // Serialize config with header comment
    let yaml = serde_yml::to_string(config)
        .context("Failed to serialize config")?;
    let contents = format!("# Auto-generated by bulker. Edit to customize.\n{}", yaml);
    std::fs::write(config_path, &contents)
        .with_context(|| format!("Failed to write config: {}", config_path.display()))?;

    Ok(())
}

/// Default config file location: ~/.config/bulker/bulker_config.yaml
pub fn default_config_path() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"));
    config_dir.join("bulker").join("bulker_config.yaml")
}

/// Expand environment variables and ~ in a path string.
pub fn expand_path(s: &str) -> String {
    let mut result = s.to_string();

    // Expand ~
    if result.starts_with('~') {
        if let Some(home) = std::env::var("HOME").ok().or_else(|| dirs::home_dir().map(|p| p.to_string_lossy().to_string())) {
            result = result.replacen('~', &home, 1);
        }
    }

    // Expand ${VAR} and $VAR patterns
    let mut output = String::with_capacity(result.len());
    let mut chars = result.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '$' {
            if chars.peek() == Some(&'{') {
                chars.next(); // consume '{'
                let var_name: String = chars.by_ref().take_while(|c| *c != '}').collect();
                if let Ok(val) = std::env::var(&var_name) {
                    output.push_str(&val);
                }
            } else {
                let mut var_name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        var_name.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !var_name.is_empty() {
                    if let Ok(val) = std::env::var(&var_name) {
                        output.push_str(&val);
                    }
                }
            }
        } else {
            output.push(ch);
        }
    }

    output
}

/// Make a path absolute, resolving relative to `rel_dir` if provided.
#[cfg(test)]
pub fn mkabs(path: &str, rel_dir: Option<&Path>) -> PathBuf {
    let expanded = expand_path(path);
    let p = PathBuf::from(&expanded);
    if p.is_absolute() {
        p
    } else if let Some(base) = rel_dir {
        base.join(p)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_host_network() {
        // On Linux (our CI), should be true
        #[cfg(not(target_os = "macos"))]
        assert!(default_host_network());
        #[cfg(target_os = "macos")]
        assert!(!default_host_network());
    }

    #[test]
    fn test_default_system_volumes() {
        #[cfg(not(target_os = "macos"))]
        assert!(default_system_volumes());
        #[cfg(target_os = "macos")]
        assert!(!default_system_volumes());
    }

    #[test]
    fn test_expand_path_home() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expand_path("~/foo"), format!("{}/foo", home));
    }

    #[test]
    fn test_expand_path_env_var() {
        let _guard = crate::test_util::EnvGuard::set("BULKER_TEST_VAR", "testval");
        assert_eq!(expand_path("${BULKER_TEST_VAR}/bar"), "testval/bar");
        assert_eq!(expand_path("$BULKER_TEST_VAR/bar"), "testval/bar");
    }

    #[test]
    fn test_mkabs_absolute() {
        let result = mkabs("/absolute/path", None);
        assert_eq!(result, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn test_mkabs_relative() {
        let result = mkabs("relative", Some(Path::new("/base")));
        assert_eq!(result, PathBuf::from("/base/relative"));
    }

    #[test]
    fn test_bulker_config_default_has_sensible_values() {
        let config = BulkerConfig::default();
        assert!(!config.bulker.container_engine.is_empty());
        assert_eq!(config.bulker.default_namespace, "bulker");
        assert_eq!(config.bulker.registry_url, "http://hub.bulker.io/");
        assert!(!config.bulker.shell_path.is_empty());
        assert_eq!(config.bulker.rcfile, "start.sh");
        assert_eq!(config.bulker.rcfile_strict, "start_strict.sh");
        assert_eq!(config.bulker.volumes, vec!["$HOME"]);
        assert_eq!(config.bulker.envvars, vec!["DISPLAY"]);
    }

    #[test]
    fn test_load_config_explicit_path_nonexistent_file() {
        // Passing an explicit path that doesn't exist should error
        let result = load_config(Some("/nonexistent/path/config.yaml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_config_explicit_path_existing_file() {
        // Write a minimal config, then load it by explicit path
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("test_config.yaml");
        let config = BulkerConfig::default();
        cache_config_to_disk(&config, &config_path).unwrap();

        let result = load_config(Some(config_path.to_str().unwrap()));
        assert!(result.is_ok());
        let (loaded, path) = result.unwrap();
        assert_eq!(loaded.bulker.default_namespace, "bulker");
        assert_eq!(path.unwrap(), config_path);
    }

    #[test]
    fn test_cache_config_to_disk_writes_file_and_templates() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("bulker_config.yaml");
        let config = BulkerConfig::default();

        cache_config_to_disk(&config, &config_path).unwrap();

        // Config file should exist
        assert!(config_path.exists());
        let contents = std::fs::read_to_string(&config_path).unwrap();
        assert!(contents.starts_with("# Auto-generated by bulker."));

        // Templates should exist
        let templates_dir = tmpdir.path().join("templates");
        assert!(templates_dir.join("start.sh").exists());
        assert!(templates_dir.join("start_strict.sh").exists());
        assert!(templates_dir.join("docker_executable.tera").exists());
    }

    #[test]
    fn test_null_container_engine_sanitized() {
        let yaml = "bulker:\n  container_engine: null\n";
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.yaml");
        std::fs::write(&config_path, yaml).unwrap();

        let config = BulkerConfig::from_file(&config_path).unwrap();
        assert_ne!(config.bulker.container_engine, "null");
        assert!(!config.bulker.container_engine.is_empty());
    }

    #[test]
    fn test_null_engine_path_sanitized() {
        let yaml = "bulker:\n  engine_path: null\n";
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.yaml");
        std::fs::write(&config_path, yaml).unwrap();

        let config = BulkerConfig::from_file(&config_path).unwrap();
        assert!(config.bulker.engine_path.is_none());
    }

    #[test]
    fn test_both_null_engine_fields_sanitized() {
        let yaml = "bulker:\n  container_engine: null\n  engine_path: null\n";
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.yaml");
        std::fs::write(&config_path, yaml).unwrap();

        let config = BulkerConfig::from_file(&config_path).unwrap();
        assert_ne!(config.bulker.container_engine, "null");
        assert!(config.bulker.engine_path.is_none());
        // engine_path() should return the sanitized container_engine, not "null"
        assert_ne!(config.engine_path(), "null");
    }

    #[test]
    fn test_normal_config_unchanged_by_sanitize() {
        let yaml = "bulker:\n  container_engine: docker\n  engine_path: /usr/bin/docker\n";
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("config.yaml");
        std::fs::write(&config_path, yaml).unwrap();

        let config = BulkerConfig::from_file(&config_path).unwrap();
        assert_eq!(config.bulker.container_engine, "docker");
        assert_eq!(config.bulker.engine_path.as_deref(), Some("/usr/bin/docker"));
    }
}
