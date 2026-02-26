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
eval "$(bulkers init-shell bash)"   # for bash
eval "$(bulkers init-shell zsh)"    # for zsh
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

## macOS notes

On Linux, bulkers adds `--network=host` and mounts system volumes (`/etc/passwd`, etc.)
by default. On macOS these are skipped, since Docker Desktop runs containers in a VM
where host networking and system volume mounts don't work as expected.

These defaults are set in `bulker_config.yaml` via `host_network` and `system_volumes`
keys. You can override them:

    bulkers config set host_network=true    # force host networking
    bulkers config set system_volumes=true  # force system volume mounts

For services that need port access on macOS, use explicit port mappings in dockerargs:

    commands:
    - command: postgres
      docker_image: postgres:latest
      dockerargs: "-p 5432:5432"

On Linux, this isn't needed — containers can bind ports directly via host networking.

## Debugging

Print the docker command that bulkers generates without running it:

    bulkers exec -p local/bedbase-test -- postgres
    bulkers exec --print-command local/bedbase-test -- postgres

Output goes to stdout, so it can be piped or saved:

    bulkers exec -p local/bedbase-test -- postgres | pbcopy

When using an activated environment, set the env var directly:

    BULKER_PRINT_COMMAND=1 samtools view input.bam

## Interactive container shells

Every command shimlink has a corresponding `_command` variant (prefixed with underscore)
that drops you into an interactive bash shell inside the container:

    bulkers activate local/bedbase-test
    _postgres    # opens bash inside the postgres container

This is useful for debugging — you can inspect the container filesystem, check installed
packages, or run the command manually with different arguments.

These `_command` shimlinks are created automatically for every command in the manifest.
They're available whenever a crate is activated.

## Running services

Bulkers is designed for CLI-style commands (run, get output, exit). For long-running
services like databases, run in a separate terminal:

    bulkers exec local/bedbase-test -- postgres

Or detach with the existing docker args escape hatch:

    BULKER_EXTRA_DOCKER_ARGS="-d --name my-postgres" bulkers exec local/bedbase-test -- postgres

Then manage with docker directly:

    docker logs my-postgres
    docker stop my-postgres

For multi-service setups (app + database + cache), use docker compose instead — it
handles networking, health checks, and dependency ordering that bulkers intentionally
does not.

### Persistent data

`--rm` is always set, so container data is ephemeral by default. For data that
should survive restarts, add a named volume in dockerargs:

    commands:
    - command: postgres
      docker_image: postgres:16
      no_user: true
      dockerargs: "-v pgdata:/var/lib/postgresql/data -p 5432:5432 -e POSTGRES_PASSWORD=dev"
