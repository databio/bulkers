# bulkers

Multi-container environment manager.

## Install

```bash
curl -sL https://raw.githubusercontent.com/databio/bulkers/master/install.sh | bash
```

This downloads the binary and adds a shell function to your `~/.bashrc` (or `~/.zshrc`) that enables `bulkers activate` to modify your current shell and `bulkers deactivate` to restore it.

### Manual install

1. Download binary:

```bash
# Linux x86_64
curl -sL https://github.com/databio/bulkers/releases/latest/download/bulkers-Linux-musl-x86_64.tar.gz | tar xz && mv bulkers ~/.local/bin/

# macOS Apple Silicon
curl -sL https://github.com/databio/bulkers/releases/latest/download/bulkers-macOS-arm64.tar.gz | tar xz && mv bulkers ~/.local/bin/

# macOS Intel
curl -sL https://github.com/databio/bulkers/releases/latest/download/bulkers-macOS-x86_64.tar.gz | tar xz && mv bulkers ~/.local/bin/
```

2. Add to `~/.bashrc` (or `~/.zshrc`):

```bash
# >>> bulkers initialize >>>
bulkers() {
  case "$1" in
    activate)
      shift
      _BULKER_OLD_PS1="$PS1"
      eval "$(\command bulkers activate --echo "$@")"
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
        unset BULKERCRATE BULKERPATH BULKERPROMPT BULKERSHELLRC BULKER_ORIG_PATH _BULKER_OLD_PS1
      fi
      ;;
    *)
      \command bulkers "$@"
      ;;
  esac
}
# <<< bulkers initialize <<<
```

Or build from source: `cargo install --path .`

## Usage

Bulkers organizes commands into three groups:

### Daily use (top-level)

```bash
bulkers activate <crate>           # shell function: put crate commands on PATH
bulkers deactivate                 # shell function: restore original PATH
bulkers exec <crate> -- <cmd>      # run one command in a crate environment
```

### Crate management

```bash
bulkers crate install <cratefile>  # install from registry shorthand, URL, or local file
bulkers crate uninstall <name>     # remove crate from disk and config
bulkers crate update [name]        # re-fetch and rebuild crate(s)
bulkers crate list                 # list installed crates
bulkers crate inspect <name>       # show commands available in a crate
```

### Configuration

```bash
bulkers config init                # create new config file
bulkers config show                # print current config
bulkers config get <key>           # get a config value
bulkers config set <key>=<value>   # set a config value
```

## Crate format reference

```
namespace/crate:tag    Full path (e.g., databio/pepatac:1.0.13)
crate                  Uses default namespace "bulker", tag "default"
crate1,crate2          Activate multiple crates together
./path/to/file.yaml    Local cratefile
https://url/file.yaml  Remote cratefile
```

## Imports

Cratefiles can import other crates. Imports are resolved at runtime (activate/exec time), not install time. This means updating an imported crate automatically propagates to all crates that import it.

```yaml
manifest:
  name: my-pipeline
  imports:
  - bulker/samtools
  - bulker/bedtools
  commands:
  - command: my-tool
    docker_image: my-org/my-tool:latest
```

When you `bulkers activate` a crate with imports, the imported crate commands are automatically added to PATH.

## AI-friendly use

The shell function (`bulkers activate`/`bulkers deactivate`) modifies the current shell, which requires an interactive session with the function loaded. For AI agents, scripts, and non-interactive contexts, use `bulkers exec` instead:

```bash
# Run a single command in a crate environment (no shell function needed)
bulkers exec bulker/demo -- cowsay hello

# Run multiple commands
bulkers exec databio/pepatac:1.0.13 -- samtools view -h input.bam

# Strict mode (only crate commands on PATH)
bulkers exec -s bulker/demo -- cowsay hi
```

`bulkers exec` is a binary command that works everywhere — CI pipelines, cron jobs, subprocess calls, AI agent tool use. No shell function or `eval` required.
