# Development Guide

## Migration Status

AxonHub is in an additive Go-to-Rust backend migration.

- **Current full backend:** legacy Go service under `cmd/axonhub/main.go`, `conf/conf.go`, and `internal/server/`
- **Rust migration slice:** Cargo workspace rooted at `Cargo.toml`
- **What the Rust slice implements today:** config loading, CLI compatibility, `/health`, SQLite-scoped `GET /admin/system/status` and `POST /admin/system/initialize`, the practical OpenAI-compatible `/v1` subset (`/v1/models`, `/v1/chat/completions`, `/v1/responses`, `/v1/embeddings`) with auth/context, routing, and SQLite persistence for the migrated path, plus explicit `501 Not Implemented` stubs for unported HTTP families
- **What is not migrated yet:** GraphQL, the broader admin plane, non-target provider wrappers/families, multi-dialect parity beyond SQLite, and full API parity

Use the Go backend or the released Docker/binary artifacts when you need the full product surface. Use the Rust workspace when working on the migration itself.

## Architecture Overview

AxonHub remains a unified AI gateway with a bidirectional request/response transformation pipeline between client SDKs and upstream model providers.

<div align="center">
  <img src="../../transformation-flow.svg" alt="AxonHub Transformation Flow" width="900"/>
</div>

The migration does **not** change the product goal. It changes the implementation strategy:

- preserve the existing operator-facing contract first,
- port behavior slice by slice,
- keep unported surfaces explicit instead of faking parity.

## Technology Stack

### Backend

- **Stable implementation:** Go 1.26+, Gin, Ent, gqlgen, FX
- **Migration slice:** Rust 1.78+, Tokio, Axum, Serde, Cargo workspace with workspace dependencies

### Frontend

- React 19
- TypeScript
- Tailwind CSS
- TanStack Router
- Zustand

## Prerequisites

- Rust 1.78+
- Go 1.26+ when working on legacy backend code
- Node.js 18+ and pnpm
- Git

## Repository Layout

### Rust Workspace

- `Cargo.toml` — workspace root and shared dependency versions
- `apps/axonhub-server` — Rust `axonhub` binary
- `crates/axonhub-config` — shared config contract, defaults, env overrides, preview/get helpers
- `crates/axonhub-http` — Axum router with `/health`, SQLite-scoped bootstrap/system routes, migrated OpenAI-compatible `/v1` routes, and truthful `501` route stubs for unported families

### Legacy Go Backend

- `cmd/axonhub/main.go` — current CLI/server contract
- `conf/conf.go` — config defaults and compatibility contract
- `internal/server/` — current full HTTP surface
- `internal/server/gql/` — GraphQL schema and resolvers
- `internal/ent/` — Ent models, schema, and migrations

## Rust Migration Slice Workflow

Run the Rust CLI from the workspace root:

```bash
cargo run -p axonhub-server -- help
cargo run -p axonhub-server -- config preview
cargo run -p axonhub-server -- config validate
cargo run -p axonhub-server -- config get server.port
cargo run -p axonhub-server --
```

Current Rust behavior is intentionally limited:

- `/health` returns a truthful health payload
- `/admin/system/status` and `/admin/system/initialize` work on the supported SQLite migration path
- `/v1/models`, `/v1/chat/completions`, `/v1/responses`, and `/v1/embeddings` run through the practical migrated Rust slice with auth/context, routing, and SQLite-backed persistence side effects
- `/admin/*`, non-target `/v1/*`, `/anthropic/v1/*`, `/jina/v1/*`, `/doubao/v3/*`, `/gemini/*`, `/v1beta/*`, `/openapi/*`, and other unported families return structured `501 Not Implemented` JSON
- config file paths and `AXONHUB_*` env keys mirror the first shared contract from `conf/conf.go`

## Frontend Development

```bash
cd frontend
pnpm install
pnpm dev
```

The frontend development server runs at `http://localhost:5173` and proxies to whichever backend you are using locally.

## Legacy Go Workflow

When you need current production behavior, work in the legacy Go backend.

Typical cases include:

- GraphQL and admin flows
- Ent schema and database-backed services
- JWT/API-key auth and middleware
- provider orchestration and outbound transformers

Legacy Go commands are still relevant **only** when touching those areas:

```bash
go test ./...
make generate
```

Use `make generate` when changing Ent or GraphQL schema inputs that own generated code.

## Verification

Recommended verification commands depend on which backend surface you changed:

### Rust slice

```bash
cargo check --workspace
cargo test --workspace
```

### Legacy Go backend

```bash
go test ./...
```

### Frontend

```bash
cd frontend
pnpm build
```

## Adding a Channel

New provider channels are still added through the legacy Go backend plus frontend configuration until provider routing is migrated.

1. **Extend the channel enum in the Ent schema**
   - Add the provider key to `field.Enum("type")` in [internal/ent/schema/channel.go](../../../internal/ent/schema/channel.go)
   - Regenerate legacy artifacts when needed

2. **Wire the outbound transformer in Go**
   - Update `ChannelService.buildChannel`
   - Add or extend the transformer implementation under the existing Go backend

3. **Register provider metadata in the frontend**
   - Update [frontend/src/features/channels/data/config_providers.ts](../../../frontend/src/features/channels/data/config_providers.ts)
   - Keep `CHANNEL_CONFIGS` and helper lookups aligned

4. **Sync frontend schema and presentation**
   - Update [frontend/src/features/channels/data/schema.ts](../../../frontend/src/features/channels/data/schema.ts)
   - Update [frontend/src/features/channels/data/constants.ts](../../../frontend/src/features/channels/data/constants.ts)

5. **Add i18n strings**
   - Update [frontend/src/locales/en.json](../../../frontend/src/locales/en.json)
   - Update [frontend/src/locales/zh.json](../../../frontend/src/locales/zh.json)
