# ULW Rust Migration Tracker

## Scope

- Goal: continue the additive Go-to-Rust migration without overstating parity.
- Current priority: CI/CD, Docker, and build-related flows.
- Constraint: preserve truthful operator behavior while the legacy Go backend remains the full-featured implementation.

## Acceptance Criteria

- Automation assets prefer the Rust workspace where the current migration slice is the intended target.
- Docker and build entrypoints are aligned with the Rust `axonhub` binary where appropriate.
- Release and workflow language stays honest about the migration state.
- Verification includes direct command output for the updated automation paths.

## Progress

- [x] Inventory current CI/CD, Docker, and build assets.
- [x] Update prioritized automation files for Rust-first flow.
- [x] Verify changed paths with direct commands and targeted diagnostics.

## Notes

- Track only completed migration slices here; do not imply unsupported backend parity.
- Added a dedicated `Dockerfile.rust`, `docker-compose.rust.yml`, Rust-specific release assets, and Rust-tagged Docker publish flow while keeping the Go/full-product path as the default delivery path.
- Patched release automation to avoid the broken `git diff VERSION` pathspec and tightened deploy scripts so they continue preferring legacy full-product archives instead of accidentally selecting Rust migration-slice assets.
- Local verification completed with `cargo build --locked --release -p axonhub-server`, Rust CLI smoke checks (`help`, `build-info`, `config preview`), YAML parsing for workflow/compose files, and shell syntax checks for deploy scripts.
- Docker CLI is unavailable in the local environment, so Docker image build/runtime verification remains workflow-level and file-level rather than locally executed container QA.
