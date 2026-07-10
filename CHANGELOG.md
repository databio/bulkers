# Changelog

## [0.0.16] - 2026-07-10

### Fixed

- Multi-crate activation (`bulker activate crateA,crateB`) now resolves
  commands from every activated crate. Previously `BULKERCRATE` recorded only
  the first crate, so shims for later crates' unique commands were on `PATH`
  but failed at exec with "command not found in crate". `activate` and `exec`
  now export all activated crates comma-joined, and the shim resolver searches
  every activated crate plus its imports.

## [0.0.15] - 2026-04-24

### Added

- `entrypoint` manifest field on `PackageCommand`. When set, bulker emits
  `--entrypoint=<value>` under Docker and uses `<value>` as the exec command
  under Apptainer. Unifies the command-resolution path across engines.

### Fixed

- Deprecation warnings emitted from the shimlink-dispatch path now actually
  reach stderr. `env_logger` is initialized in the shim branch of `main`;
  previously `log::warn!` calls fired into a null logger when bulker was
  invoked via a shim symlink (i.e. the normal execution path).

### Deprecated

- `docker_command`, `apptainer_command`, `singularity_command` — use
  `entrypoint` instead. A deprecation warning is logged on every use; these
  fields will be removed in the next minor bump.
- `--entrypoint` as a flag inside `docker_args` — use the `entrypoint`
  manifest field instead. A warning is logged when `--entrypoint` is seen
  inside `docker_args`.
