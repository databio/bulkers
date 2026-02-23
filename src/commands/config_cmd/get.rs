use anyhow::{Result, bail};
use clap::{Arg, ArgMatches, Command};

use crate::config::{BulkerConfig, select_config};

pub fn create_cli() -> Command {
    Command::new("get")
        .about("Get a configuration value")
        .after_help("\
EXAMPLES:
  bulkers config get envvars
  bulkers config get container_engine
  bulkers config get shell_path

SUPPORTED KEYS:
  container_engine, default_crate_folder, default_namespace, registry_url,
  shell_path, shell_rc, envvars, volumes, shell_prompt, singularity_image_folder")
        .arg(
            Arg::new("key")
                .required(true)
                .help("Configuration key to read"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let config_path = select_config(matches.get_one::<String>("config").map(|s| s.as_str()))?;
    let config = BulkerConfig::from_file(&config_path)?;
    let key = matches.get_one::<String>("key").unwrap();

    match key.as_str() {
        "container_engine" => println!("{}", config.bulker.container_engine),
        "default_crate_folder" => println!("{}", config.bulker.default_crate_folder),
        "default_namespace" => println!("{}", config.bulker.default_namespace),
        "registry_url" => println!("{}", config.bulker.registry_url),
        "shell_path" => println!("{}", config.bulker.shell_path),
        "shell_rc" => println!("{}", config.bulker.shell_rc),
        "envvars" => {
            for v in &config.bulker.envvars {
                println!("{}", v);
            }
        }
        "volumes" => {
            for v in &config.bulker.volumes {
                println!("{}", v);
            }
        }
        "shell_prompt" => {
            if let Some(ref p) = config.bulker.shell_prompt {
                println!("{}", p);
            }
        }
        "singularity_image_folder" => {
            if let Some(ref f) = config.bulker.singularity_image_folder {
                println!("{}", f);
            }
        }
        _ => bail!("Unknown config key: '{}'. Supported keys: container_engine, default_crate_folder, default_namespace, registry_url, shell_path, shell_rc, envvars, volumes, shell_prompt, singularity_image_folder", key),
    }

    Ok(())
}
