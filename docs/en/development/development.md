# Development Guide

## Migration Status

AxonHub is in an additive Go-to-Rust backend migration.

- **Current full backend:** legacy Go service under `cmd/axonhub/main.go`, `conf/conf.go`, and `internal/server/`
- **Rust migration slice:** Cargo workspace rooted at `Cargo.toml`

### Rust Slice Current Coverage (Verified)

The Rust slice implements the following verified SQLite- and PostgreSQL-backed surface:

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

### Rust Slice Remaining Work (Explicit Buckets)

**Next (high-priority, near-term):**

- Image generation endpoints (`/v1/images/generations`, `/v1/images/edits`)
- RBAC/permission system migration (internal/scopes)
- Core business logic surfaces (internal/server/biz): channel/model management, request lifecycle, usage/cost, trace/thread
- Transformer/pipeline migration (llm/transformer, llm/pipeline): provider orchestration, outbound adapters
- Model association/fetching parity
- System onboarding completeness (bootstrap flows, default data)
- OAuth parity verification (complete OAuth flow implementations)
- MySQL integration verification completion

**Later (medium-priority, mid-term):**

- AiSDK compatibility (complete Vercel AI SDK protocol)
- Full admin GraphQL write operations and advanced queries
- Advanced/enterprise features: prompt protection, provider quota management, circuit breakers, channel auto-disable
- Config alignment: full parity with legacy Go backend configuration options
- Broader test parity: integration test coverage matching the Go suite
- Additional provider-specific features (Gemini tools, Anthropic extensions)

**Deferred with 501 (explicit boundaries):**

- Operational/background items that remain Go-only until separate migration gates
- Legacy-only database dialects (TiDB, Neon DB) - remain on Go backend
- Helm Kubernetes deployment path (Go backend only)
- Full legacy Go API surface that is not part of the targeted Rust slice

### Unported HTTP Families (Truthful 501)

Route families outside the verified scope return structured `501 Not Implemented` JSON:

- `/v1/images/generations`, `/v1/images/edits` (image generation)
- `/admin/*` write operations (user management, project creation, role assignment, etc.)
- Non-target provider wrappers not yet migrated
- Realtime API endpoints (representative `/v1/realtime` traffic stays on explicit `/v1/*` `501` boundaries)
- Vercel AI SDK protocol marker headers on `/v1/*` requests (for example `X-Vercel-Ai-Ui-Message-Stream: v1` or `X-Vercel-AI-Data-Stream: v1`)
- Full admin plane beyond read operations

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
- **Migration slice:** Rust 1.78+, Tokio, Actix Web, Serde, Cargo workspace with shared dependencies

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
- `crates/axonhub-http` — Actix router with `/health`, verified SQLite- and PostgreSQL-backed bootstrap/system routes, migrated OpenAI-compatible `/v1` routes, and truthful `501` route stubs for unported families

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
- `/admin/system/status` and `/admin/system/initialize` work on the supported SQLite- and PostgreSQL-backed migration paths
- `/v1/models`, `/v1/chat/completions`, `/v1/responses`, and `/v1/embeddings` run through the practical migrated Rust slice with auth/context, routing, and SQLite- and PostgreSQL-backed persistence side effects
- MySQL uses the same SeaORM-backed repository seam, but full Rust-side integration verification is still pending; TiDB and Neon DB remain Go-only
- `/admin/*` write operations, non-target `/v1/*` routes such as image generation, and other still-unported families remain explicit structured `501 Not Implemented` JSON boundaries
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
