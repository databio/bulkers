use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::manifest::CrateVars;

const BULKERCFG_ENV: &str = "BULKERCFG";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CrateEntry {
    pub path: String,
    #[serde(default)]
    pub imports: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BulkerConfig {
    pub bulker: BulkerSettings,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BulkerSettings {
    pub container_engine: String,
    pub default_crate_folder: String,
    pub default_namespace: String,
    #[serde(default = "default_registry_url")]
    pub registry_url: String,
    pub shell_path: String,
    #[serde(default = "default_shell_rc")]
    pub shell_rc: String,
    pub executable_template: String,
    pub shell_template: String,
    pub build_template: String,
    pub rcfile: String,
    pub rcfile_strict: String,
    #[serde(default = "default_volumes")]
    pub volumes: Vec<String>,
    #[serde(default = "default_envvars")]
    pub envvars: Vec<String>,
    #[serde(default)]
    pub crates: Option<BTreeMap<String, BTreeMap<String, BTreeMap<String, CrateEntry>>>>,
    #[serde(default)]
    pub tool_args: Option<serde_yaml::Value>,
    #[serde(default)]
    pub shell_prompt: Option<String>,
    #[serde(default)]
    pub apptainer_image_folder: Option<String>,
}

fn default_registry_url() -> String {
    "http://hub.bulker.io/".to_string()
}

fn default_shell_rc() -> String {
    "$HOME/.bashrc".to_string()
}

fn default_volumes() -> Vec<String> {
    vec!["$HOME".to_string()]
}

fn default_envvars() -> Vec<String> {
    vec!["DISPLAY".to_string()]
}

impl BulkerConfig {
    pub fn from_file(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;
        let mut config: BulkerConfig = serde_yaml::from_str(&contents)
            .with_context(|| format!("Failed to parse config: {}", path.display()))?;

        // Expand env vars in key paths
        config.bulker.default_crate_folder = expand_path(&config.bulker.default_crate_folder);
        config.bulker.shell_path = expand_path(&config.bulker.shell_path);
        config.bulker.shell_rc = expand_path(&config.bulker.shell_rc);
        if let Some(ref folder) = config.bulker.apptainer_image_folder {
            config.bulker.apptainer_image_folder = Some(expand_path(folder));
        }

        Ok(config)
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        let yaml = serde_yaml::to_string(self)
            .context("Failed to serialize config")?;
        std::fs::write(path, &yaml)
            .with_context(|| format!("Failed to write config: {}", path.display()))?;
        Ok(())
    }

    /// Get or initialize the crates map, returning a mutable reference.
    pub fn crates_mut(&mut self) -> &mut BTreeMap<String, BTreeMap<String, BTreeMap<String, CrateEntry>>> {
        self.bulker.crates.get_or_insert_with(BTreeMap::new)
    }

    /// Get a reference to the crates map (empty if None).
    pub fn crates(&self) -> &BTreeMap<String, BTreeMap<String, BTreeMap<String, CrateEntry>>> {
        static EMPTY: std::sync::LazyLock<BTreeMap<String, BTreeMap<String, BTreeMap<String, CrateEntry>>>> =
            std::sync::LazyLock::new(BTreeMap::new);
        self.bulker.crates.as_ref().unwrap_or(&EMPTY)
    }

    /// Get a CrateEntry by cratevars.
    pub fn get_crate_entry(&self, cratevars: &CrateVars) -> Option<&CrateEntry> {
        self.crates()
            .get(&cratevars.namespace)?
            .get(&cratevars.crate_name)?
            .get(&cratevars.tag)
    }

    /// Get a mutable CrateEntry by cratevars.
    pub fn get_crate_entry_mut(&mut self, cratevars: &CrateVars) -> Option<&mut CrateEntry> {
        self.crates_mut()
            .get_mut(&cratevars.namespace)?
            .get_mut(&cratevars.crate_name)?
            .get_mut(&cratevars.tag)
    }
}

/// Select the config file path: explicit arg > $BULKERCFG > default location.
pub fn select_config(arg: Option<&str>) -> Result<PathBuf> {
    if let Some(path) = arg {
        let p = PathBuf::from(expand_path(path));
        if p.exists() {
            return Ok(p);
        }
        bail!("Config file not found: {}", p.display());
    }

    if let Ok(env_path) = std::env::var(BULKERCFG_ENV) {
        let p = PathBuf::from(expand_path(&env_path));
        if p.exists() {
            return Ok(p);
        }
        bail!("Config from ${} not found: {}", BULKERCFG_ENV, p.display());
    }

    let default_path = default_config_path();
    if default_path.exists() {
        return Ok(default_path);
    }

    bail!(
        "No bulker config found. Set ${}, pass -c, or run `bulkers config init`.",
        BULKERCFG_ENV
    )
}

/// Default config file location: ~/.bulker/bulker_config.yaml
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
    fn test_expand_path_home() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expand_path("~/foo"), format!("{}/foo", home));
    }

    #[test]
    fn test_expand_path_env_var() {
        // SAFETY: test runs single-threaded
        unsafe { std::env::set_var("BULKER_TEST_VAR", "testval"); }
        assert_eq!(expand_path("${BULKER_TEST_VAR}/bar"), "testval/bar");
        assert_eq!(expand_path("$BULKER_TEST_VAR/bar"), "testval/bar");
        unsafe { std::env::remove_var("BULKER_TEST_VAR"); }
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
}
