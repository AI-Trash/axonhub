# Development Guide

## Backend Contract Status

AxonHub's canonical backend implementation is Rust.

- **Canonical backend path:** Cargo workspace rooted at `Cargo.toml`
- **Legacy reference/oracle tree:** `cmd/axonhub/main.go`, `conf/conf.go`, and `internal/server/`

### Current Rust Coverage (Verified)

The Rust backend implements the following verified SQLite- and PostgreSQL-backed surface:

- **Config & CLI**: config loading, CLI compatibility (`config preview`, `config validate`, `config get`, `version`, `help`)
- **Health & system**: `/health`, `GET /admin/system/status`, `POST /admin/system/initialize`
- **Identity & context**: authentication, request context, JWT handling
- **Admin read routes**: `GET /admin/requests/:request_id/content`
- **Admin GraphQL**: `POST /admin/graphql` with playground, OAuth flows (Codex, Claude Code, Antigravity, Copilot)
- **OpenAPI GraphQL**: `POST /openapi/v1/graphql` with playground
- **OpenAI-compatible `/v1` inference (standard JSON requests only)**: `/models`, `/chat/completions`, `/responses`, `/embeddings`, `/messages`, `/rerank`
- **Video generation**: `POST /v1/videos`, `GET /v1/videos/{id}`, `DELETE /v1/videos/{id}`
- **Other provider APIs**: Jina, Anthropic, Gemini, Doubao routes as configured
- **Database support**: SQLite and PostgreSQL fully verified; MySQL wired through shared SeaORM seam but full integration verification pending

### Remaining Follow-up Areas

The Rust backend remains the canonical implementation path while broader follow-up work continues around deeper RBAC/admin coverage, MySQL integration verification, additional provider-edge verification, and other non-boundary parity areas.

The legacy Go tree remains in-repo as historical reference/oracle material. It is not the current/full runtime for maintained documentation or repo guidance.

### Accepted Explicit Unsupported Boundaries (Truthful 501)

Accepted route families outside the current verified Rust surface return structured `501 Not Implemented` JSON:

- `POST /v1/images/edits`
- Realtime API endpoints (representative `/v1/realtime` traffic stays on explicit `/v1/*` `501` boundaries)
- Gemini `countTokens` requests on the accepted explicit compatibility boundary
- Vercel AI SDK protocol marker headers on `/v1/*` requests (for example `X-Vercel-Ai-Ui-Message-Stream: v1` or `X-Vercel-AI-Data-Stream: v1`)

Use the Rust backend and Rust-tagged release artifacts for the current supported product surface. Keep the legacy Go tree only for historical reference/oracle work when a task explicitly requires it.

## Architecture Overview

AxonHub remains a unified AI gateway with a bidirectional request/response transformation pipeline between client SDKs and upstream model providers.

<div align="center">
  <img src="../../transformation-flow.svg" alt="AxonHub Transformation Flow" width="900"/>
</div>

The current backend architecture does **not** change the product goal. It changes how the canonical implementation is maintained:

- preserve the operator-facing contract,
- keep accepted explicit unsupported boundaries honest,
- extend Rust behavior under parity regression gates.

## Technology Stack

### Backend

- **Canonical implementation:** Rust 1.78+, Tokio, Actix Web, Serde, Cargo workspace with shared dependencies
- **Legacy reference/oracle material:** Go 1.26+, Gin, Ent, gqlgen, FX

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
- `crates/axonhub-http` — Actix router with `/health`, verified SQLite- and PostgreSQL-backed bootstrap/system routes, current OpenAI-compatible `/v1` routes, and truthful `501` boundaries for the accepted unsupported families

### Legacy Go Backend

- `cmd/axonhub/main.go` — historical CLI/server contract reference
- `conf/conf.go` — historical config defaults and compatibility reference
- `internal/server/` — historical reference/oracle HTTP surface
- `internal/server/gql/` — GraphQL schema and resolvers
- `internal/ent/` — Ent models, schema, and migrations

## Rust Backend Workflow

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
- `/admin/system/status` and `/admin/system/initialize` work on the supported SQLite- and PostgreSQL-backed Rust paths
- `/v1/models`, `/v1/chat/completions`, `/v1/responses`, and `/v1/embeddings` run through the current Rust backend with auth/context, routing, and SQLite- and PostgreSQL-backed persistence side effects
- MySQL uses the same SeaORM-backed repository seam, but full Rust-side integration verification is still pending; TiDB and Neon DB remain legacy-reference dialect material in the Go tree
- `POST /v1/images/edits`, `/v1/realtime`, Gemini `countTokens`, and AiSDK-marked `/v1/*` requests remain explicit structured `501 Not Implemented` JSON boundaries
- config file paths and `AXONHUB_*` env keys mirror the preserved operator-facing contract from `conf/conf.go`

## Frontend Development

```bash
cd frontend
pnpm install
pnpm dev
```

The frontend development server runs at `http://localhost:5173` and proxies to whichever backend you are using locally.

## Legacy Go Reference Workflow

Work in the legacy Go tree only when a task explicitly needs historical reference/oracle material or generated artifacts that still live there.

Typical cases include:

- comparing older contract/oracle behavior during parity investigations
- touching Ent or GraphQL generated-code inputs that still live in the Go tree
- maintaining historical reference schemas, handlers, or compatibility notes

Legacy Go commands are still relevant **only** when touching those legacy-reference areas:

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

Some provider-channel additions still require touching legacy Go schema/transformer assets plus frontend configuration when those reference-owned pieces are involved.

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
