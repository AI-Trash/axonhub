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

- [ ] Inventory current CI/CD, Docker, and build assets.
- [ ] Update prioritized automation files for Rust-first flow.
- [ ] Verify changed paths with direct commands and targeted diagnostics.

## Notes

- Track only completed migration slices here; do not imply unsupported backend parity.
