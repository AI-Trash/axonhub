# Rust vs Go backend parity plan

## Goal

Analyze the current Rust-vs-Go backend gap, then close the highest-impact gaps first with priority on:

1. Features that block the Rust server from being practically usable
2. Codex / OpenAI compatibility
3. Broader Go parity afterward

## Implementation Tasks

- [x] Inventory the current Rust-supported backend surface and explicit unsupported boundaries
- [x] Identify the highest-impact Rust-vs-Go parity gaps affecting OpenAI/Codex flows
- [x] Implement Rust support for OpenAI Responses `previous_response_id` chaining
- [x] Implement Rust support for retrieving Responses objects by response ID if missing
- [x] Verify Codex trace/session continuity behavior on the Rust path
- [x] Identify and fix additional runtime blockers that prevent the Rust server from being practically usable
- [x] Reassess remaining Rust-vs-Go parity gaps and select the next highest-impact slice

## Current Next Slice Decision

- [ ] Implement active `/admin/graphql` `updateVideoStorageSettings` parity on the Rust SeaORM subset dispatcher.
  - Why this slice next:
    - It is actively used by the System → Storage UI via `useUpdateVideoStorageSettings()` in `frontend/src/features/system/data/system.ts` and the save button in `frontend/src/features/system/components/video-storage-settings.tsx`.
    - The active Rust subset dispatcher in `apps/axonhub-server/src/foundation/graphql.rs` already supports the `videoStorageSettings` read path, but still has no `updateVideoStorageSettings` mutation branch, so saving falls through to the generic not-implemented `/admin/graphql` response.
    - It is smaller than the remaining leftovers because the backing stored settings type, defaults, and read helper already exist; only the active write path is missing.
    - It is higher-value than dormant dashboard leftovers because it fixes a live broken save action on an active admin page today.
  - Hidden complexity to preserve truthfully from Go:
    - Preserve partial-update merge behavior for optional fields instead of replacing the whole record blindly.
    - Normalize non-positive `scanIntervalMinutes` / `scanLimit` back to defaults, matching the existing stored settings defaults.
    - Add new write-side validation/persistence logic in `apps/axonhub-server/src/foundation/admin_operational.rs` (for example `SeaOrmOperationalService::update_video_storage_settings(...)`), modeled on Go `internal/server/biz/system.go`, so video storage cannot be pointed at invalid database/primary storage and still returns field-level GraphQL errors rather than a 501.
  - References:
    - Go schema/resolver: `internal/server/gql/system.graphql`, `internal/server/gql/system.resolvers.go`
    - Go service semantics: `internal/server/biz/system.go`
    - Rust active dispatcher: `apps/axonhub-server/src/foundation/graphql.rs`
    - Rust stored settings / operational read path to extend: `apps/axonhub-server/src/foundation/admin.rs`, `apps/axonhub-server/src/foundation/admin_operational.rs`
    - Frontend consumers: `frontend/src/features/system/data/system.ts`, `frontend/src/features/system/components/video-storage-settings.tsx`

## Final Wave

- [ ] F1: Code review of changed Rust OpenAI/Codex/runtime paths approves scope and correctness
- [ ] F2: Targeted verification for changed flows passes
- [ ] F3: Broad regression verification passes
- [ ] F4: Final parity summary truthfully reflects what is now implemented vs still deferred

## QA Scenarios For Current Next Slice

- [ ] `updateVideoStorageSettings` success path
  - Tool: Rust targeted tests in `apps/axonhub-server/src/foundation/tests.rs`
  - Steps: seed an active non-database data storage, call `updateVideoStorageSettings(input:{ enabled, dataStorageID, scanIntervalMinutes, scanLimit })`, then query `videoStorageSettings`.
  - Expected: mutation returns `true`, readback matches the stored values, and the `system_video_storage_settings` record is updated.

- [ ] `updateVideoStorageSettings` normalization path
  - Tool: Rust targeted tests in `apps/axonhub-server/src/foundation/tests.rs`
  - Steps: mutate with non-positive `scanIntervalMinutes` / `scanLimit`.
  - Expected: stored/readback values normalize to the default video storage settings instead of persisting invalid zero or negative values.

- [ ] `updateVideoStorageSettings` validation boundaries
  - Tool: Rust targeted tests in `apps/axonhub-server/src/foundation/tests.rs`
  - Steps: enable video storage without `dataStorageID`, and separately target a database/invalid storage.
  - Expected: field-level GraphQL errors are returned for invalid input instead of a 501 route error.

- [ ] `updateVideoStorageSettings` permission boundary
  - Tool: Rust targeted tests in `apps/axonhub-server/src/foundation/tests.rs`
  - Steps: issue the same mutation without `write_settings`.
  - Expected: `data.updateVideoStorageSettings = null` and `permission denied`.

- [ ] Deferred siblings remain explicitly out of scope for this slice
  - Tool: Rust targeted tests or manual GraphQL assertion on the active route.
  - Steps: query `topRequestsProjects` before implementing it.
  - Expected: active Rust `/admin/graphql` still returns the generic not-implemented field response, proving this slice stays isolated.
