# bulker

Multi-container environment manager.

## Install

```bash
curl -sL https://raw.githubusercontent.com/databio/bulkers/master/install.sh | bash
```

This downloads the binary and adds a shell function to your `~/.bashrc` (or `~/.zshrc`) that enables `bulker activate` to modify your current shell and `bulker deactivate` to restore it.

### Manual install

1. Download binary:

```bash
# Linux x86_64
curl -sL https://github.com/databio/bulkers/releases/latest/download/bulker-Linux-musl-x86_64.tar.gz | tar xz && mv bulker ~/.local/bin/

# macOS Apple Silicon
curl -sL https://github.com/databio/bulkers/releases/latest/download/bulker-macOS-arm64.tar.gz | tar xz && mv bulker ~/.local/bin/

# macOS Intel
curl -sL https://github.com/databio/bulkers/releases/latest/download/bulker-macOS-x86_64.tar.gz | tar xz && mv bulker ~/.local/bin/
```

2. Add to `~/.bashrc` (or `~/.zshrc`):

```bash
eval "$(bulker init-shell bash)"   # for bash
eval "$(bulker init-shell zsh)"    # for zsh
```

Or build from source: `cargo install --path .`

### Local repo install

To install from a local clone of the repo:

```bash
cargo build --release
./install.sh
source ~/.bashrc  # or: source ~/.zshrc
```

This builds the binary, copies it to `~/.local/bin/`, and adds the shell function to your shell rc file.


## Usage

Bulker organizes commands into three groups:

### Daily use (top-level)

```bash
bulker activate <crate>           # shell function: put crate commands on PATH
bulker deactivate                 # shell function: restore original PATH
bulker exec <crate> -- <cmd>      # run one command in a crate environment
```

### Crate management

```bash
bulker crate install <cratefile>  # install from registry shorthand, URL, or local file
bulker crate uninstall <name>     # remove crate from disk and config
bulker crate update [name]        # re-fetch and rebuild crate(s)
bulker crate list                 # list installed crates
bulker crate inspect <name>       # show commands available in a crate
```

### Configuration

```bash
bulker config init                # create new config file
bulker config show                # print current config
bulker config get <key>           # get a config value
bulker config set <key>=<value>   # set a config value
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

When you `bulker activate` a crate with imports, the imported crate commands are automatically added to PATH.

## AI-friendly use

The shell function (`bulker activate`/`bulker deactivate`) modifies the current shell, which requires an interactive session with the function loaded. For AI agents, scripts, and non-interactive contexts, use `bulker exec` instead:

```bash
# Run a single command in a crate environment (no shell function needed)
bulker exec bulker/demo -- cowsay hello

# Run multiple commands
bulker exec databio/pepatac:1.0.13 -- samtools view -h input.bam

# Strict mode (only crate commands on PATH)
bulker exec -s bulker/demo -- cowsay hi
```

`bulker exec` is a binary command that works everywhere — CI pipelines, cron jobs, subprocess calls, AI agent tool use. No shell function or `eval` required.

## macOS notes

On Linux, bulker adds `--network=host` and mounts system volumes (`/etc/passwd`, etc.)
by default. On macOS these are skipped, since Docker Desktop runs containers in a VM
where host networking and system volume mounts don't work as expected.

These defaults are set in `bulker_config.yaml` via `host_network` and `system_volumes`
keys. You can override them:

    bulker config set host_network=true    # force host networking
    bulker config set system_volumes=true  # force system volume mounts

For services that need port access on macOS, use explicit port mappings in dockerargs:

    commands:
    - command: postgres
      docker_image: postgres:latest
      dockerargs: "-p 5432:5432"

On Linux, this isn't needed — containers can bind ports directly via host networking.

## Debugging

Print the docker command that bulker generates without running it:

    bulker exec -p local/bedbase-test -- postgres
    bulker exec --print-command local/bedbase-test -- postgres

Output goes to stdout, so it can be piped or saved:

    bulker exec -p local/bedbase-test -- postgres | pbcopy

When using an activated environment, set the env var directly:

    BULKER_PRINT_COMMAND=1 samtools view input.bam

## Interactive container shells

Every command shimlink has a corresponding `_command` variant (prefixed with underscore)
that drops you into an interactive bash shell inside the container:

    bulker activate local/bedbase-test
    _postgres    # opens bash inside the postgres container

This is useful for debugging — you can inspect the container filesystem, check installed
packages, or run the command manually with different arguments.

These `_command` shimlinks are created automatically for every command in the manifest.
They're available whenever a crate is activated.

## Running services

Bulker is designed for CLI-style commands (run, get output, exit). For long-running
services like databases, run in a separate terminal:

    bulker exec local/bedbase-test -- postgres

Or detach with the existing docker args escape hatch:

    BULKER_EXTRA_DOCKER_ARGS="-d --name my-postgres" bulker exec local/bedbase-test -- postgres

Then manage with docker directly:

    docker logs my-postgres
    docker stop my-postgres

For multi-service setups (app + database + cache), use docker compose instead — it
handles networking, health checks, and dependency ordering that bulker intentionally
does not.

### Persistent data

`--rm` is always set, so container data is ephemeral by default. For data that
should survive restarts, add a named volume in dockerargs:

    commands:
    - command: postgres
      docker_image: postgres:16
      no_user: true
      dockerargs: "-v pgdata:/var/lib/postgresql/data -p 5432:5432 -e POSTGRES_PASSWORD=dev"
