# CLAUDE.md

Bulker is a multi-container environment manager. It makes containerized CLI tools act like native commands on your PATH. Load a crate (a YAML manifest of containers), activate it, and every tool becomes available. Supports Docker and Apptainer.

## Core workflow

```bash
bulker config init                    # create config (detects docker/apptainer)
bulker crate install demo             # fetch and cache a crate manifest
bulker activate bulker/demo           # put crate commands on PATH
bulker deactivate                     # restore original PATH
bulker exec bulker/demo -- cowsay hi  # one-shot (no shell function needed)
```

`activate`/`deactivate` are shell functions (require `eval "$(bulker init-shell bash)"`). `exec` is a binary command that works everywhere.

## Key concept: shimlinks

Bulker uses a busybox pattern. `activate` creates a temp directory of symlinks (e.g., `samtools` -> `bulker`). When invoked via symlink, bulker detects the command name from argv[0], looks it up in the manifest, and constructs the `docker run`/`apptainer exec` command dynamically. No generated shell scripts.

## Manifest format

```yaml
manifest:
  name: my-tools
  imports:
  - bulker/samtools         # pull commands from another crate
  commands:
  - command: mytool
    docker_image: org/tool:latest
  - command: python
    docker_image: python:3.12
    volumes: ["/data:/data"]
```

## CLI command tree

- `activate <crate>` / `deactivate` — shell functions for PATH manipulation
- `exec <crate> -- <cmd>` — run one command without activating
- `crate install|list|inspect|clean` — manage cached manifests
- `config init|show|get|set` — manage configuration
- `mock run|record` — CI testing without containers
- `init-shell <shell>` — print shell function for eval
- `completions <shell>` — print shell completions

## Crate path format

```
namespace/crate:tag    Full path (e.g., databio/pepatac:1.0.13)
crate                  Uses default namespace "bulker", tag "default"
crate1,crate2          Activate multiple crates
./path/to/file.yaml    Local manifest file
```

## Architecture

| Module | Purpose |
|--------|---------|
| `shimlink.rs` | Busybox-pattern dispatch: argv[0] lookup, docker/apptainer command construction |
| `manifest_cache.rs` | Filesystem cache at ~/.config/bulker/manifests/; auto-fetch from registry |
| `activate.rs` | Create ephemeral shimlink dir in /tmp, exec subshell with modified PATH |
| `templates.rs` | Tera templates for docker/apptainer commands (executable, shell, build) |
| `imports.rs` | Recursive crate import resolution from manifest cache |
| `mock.rs` | Record real container outputs as JSON, replay via Python scripts |
| `config.rs` | YAML config with container engine, volumes, envvars, shell settings |
| `manifest.rs` | Parse crate manifests (YAML with PackageCommand structs) |

## Development

```bash
cargo build
cargo test
cargo build --release
```
