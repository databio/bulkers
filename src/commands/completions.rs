use anyhow::Result;
use clap::{Arg, ArgMatches, Command};
use clap_complete::{Shell, generate};
use std::io;

pub fn create_cli() -> Command {
    Command::new("completions")
        .about("Generate shell completions")
        .arg(
            Arg::new("shell")
                .required(true)
                .value_parser(["bash", "zsh", "fish"])
                .help("Shell type"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let shell_name = matches.get_one::<String>("shell").unwrap();
    let shell: Shell = match shell_name.as_str() {
        "bash" => Shell::Bash,
        "zsh" => Shell::Zsh,
        "fish" => Shell::Fish,
        _ => unreachable!("clap validates the value"),
    };

    let mut cmd = crate::build_parser();
    generate(shell, &mut cmd, "bulker", &mut io::stdout());
    Ok(())
}
