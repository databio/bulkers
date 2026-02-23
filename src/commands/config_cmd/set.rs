use anyhow::{Result, bail};
use clap::{Arg, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};

pub fn create_cli() -> Command {
    Command::new("set")
        .about("Set a configuration value")
        .after_help("\
EXAMPLES:
  bulkers config set container_engine=singularity
  bulkers config set envvars=HOME,DISPLAY,LANG
  bulkers config set shell_path=/bin/zsh

For list fields (envvars, volumes), use comma-separated values.")
        .arg(
            Arg::new("key_value")
                .required(true)
                .help("Key=value pair to set (e.g. container_engine=docker)"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let mut config = BulkerConfig::from_file(&config_path)?;
    let kv = matches.get_one::<String>("key_value").unwrap();

    let (key, value) = kv.split_once('=')
        .ok_or_else(|| anyhow::anyhow!("Expected key=value format, got: '{}'", kv))?;

    match key {
        "container_engine" => config.bulker.container_engine = value.to_string(),
        "default_crate_folder" => config.bulker.default_crate_folder = value.to_string(),
        "default_namespace" => config.bulker.default_namespace = value.to_string(),
        "registry_url" => config.bulker.registry_url = value.to_string(),
        "shell_path" => config.bulker.shell_path = value.to_string(),
        "shell_rc" => config.bulker.shell_rc = value.to_string(),
        "envvars" => {
            config.bulker.envvars = value.split(',').map(|s| s.trim().to_string()).collect();
        }
        "volumes" => {
            config.bulker.volumes = value.split(',').map(|s| s.trim().to_string()).collect();
        }
        "shell_prompt" => {
            config.bulker.shell_prompt = if value.is_empty() { None } else { Some(value.to_string()) };
        }
        "singularity_image_folder" => {
            config.bulker.singularity_image_folder = if value.is_empty() { None } else { Some(value.to_string()) };
        }
        _ => bail!("Unknown config key: '{}'. Supported keys: container_engine, default_crate_folder, default_namespace, registry_url, shell_path, shell_rc, envvars, volumes, shell_prompt, singularity_image_folder", key),
    }

    config.write(&config_path)?;
    println!("Set {}={}", key, value);
    Ok(())
}
