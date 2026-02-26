use anyhow::Result;
use clap::{Arg, ArgMatches, Command};

pub fn create_cli() -> Command {
    Command::new("init-shell")
        .about("Print shell function for ~/.bashrc or ~/.zshrc")
        .hide(true)
        .after_help("\
EXAMPLES:
  eval \"$(bulker init-shell bash)\"     # add to ~/.bashrc
  eval \"$(bulker init-shell zsh)\"      # add to ~/.zshrc
  bulker init-shell bash                # print the function to stdout")
        .arg(
            Arg::new("shell")
                .required(true)
                .value_parser(["bash", "zsh"])
                .help("Shell type (bash or zsh)"),
        )
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let shell = matches.get_one::<String>("shell").unwrap();

    let function = match shell.as_str() {
        "zsh" => SHELL_FUNCTION_ZSH,
        _ => SHELL_FUNCTION_BASH,
    };

    print!("{}", function);
    Ok(())
}

const SHELL_FUNCTION_BASH: &str = r#"# >>> bulker initialize >>>
bulker() {
  case "$1" in
    activate)
      shift
      _BULKER_OLD_PS1="$PS1"
      eval "$(\command bulker activate --echo "$@")"
      if [ -n "$BULKERCRATE" ]; then
        PS1="(\[\033[01;93m\]${BULKERCRATE}\[\033[00m\]) ${_BULKER_OLD_PS1}"
      fi
      ;;
    deactivate)
      if [ -n "$BULKER_ORIG_PATH" ]; then
        export PATH="$BULKER_ORIG_PATH"
        if [ -n "$_BULKER_OLD_PS1" ]; then
          PS1="$_BULKER_OLD_PS1"
        fi
        [ -d "$BULKER_SHIMDIR" ] && rm -rf "$BULKER_SHIMDIR"
        unset BULKERCRATE BULKERPATH BULKER_SHIMDIR BULKERPROMPT BULKERSHELLRC BULKER_ORIG_PATH _BULKER_OLD_PS1
      fi
      ;;
    *)
      \command bulker "$@"
      ;;
  esac
}
eval "$(\command bulker completions bash)"
# <<< bulker initialize <<<
"#;

const SHELL_FUNCTION_ZSH: &str = r#"# >>> bulker initialize >>>
bulker() {
  case "$1" in
    activate)
      shift
      _BULKER_OLD_PS1="$PS1"
      eval "$(\command bulker activate --echo "$@")"
      if [ -n "$BULKERCRATE" ]; then
        PS1="(%F{226}${BULKERCRATE}%f) ${_BULKER_OLD_PS1}"
      fi
      ;;
    deactivate)
      if [ -n "$BULKER_ORIG_PATH" ]; then
        export PATH="$BULKER_ORIG_PATH"
        if [ -n "$_BULKER_OLD_PS1" ]; then
          PS1="$_BULKER_OLD_PS1"
        fi
        [ -d "$BULKER_SHIMDIR" ] && rm -rf "$BULKER_SHIMDIR"
        unset BULKERCRATE BULKERPATH BULKER_SHIMDIR BULKERPROMPT BULKERSHELLRC BULKER_ORIG_PATH _BULKER_OLD_PS1
      fi
      ;;
    *)
      \command bulker "$@"
      ;;
  esac
}
eval "$(\command bulker completions zsh)"
# <<< bulker initialize <<<
"#;
