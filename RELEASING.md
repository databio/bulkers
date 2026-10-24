# Releasing

1. Bump `version` in `Cargo.toml`, then run `cargo test` so `Cargo.lock` picks up the new version.
2. In `CHANGELOG.md`, rename `## [Unreleased]` to `## [x.y.z] - YYYY-MM-DD`.
3. Commit ("Bump version to x.y.z, update changelog") and push to `master`.
4. Draft a GitHub release: tag `vx.y.z`, target `master`, title `vx.y.z`, notes from the
   changelog entry.
5. Publish it. The new `v*` tag triggers `build-binaries.yml`, which builds and attaches
   Linux and macOS binaries. Check that all four are attached.

Versions stay at 0.0.x: patch bumps for fixes and small features.
