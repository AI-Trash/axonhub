# AGENTS.md

This file provides guidance to AI coding assistants when working with code in this repository.

> **Detailed rules are in `.agent/rules/`** — see [Rules Index](#rules-index) below.

## Global Rules

1. Do NOT run lint or build commands unless explicitly requested by the user.
2. Do NOT restart the development server — it's already started and managed.
3. All summary files should be stored in `.agent/summary` directory if available.

## Configuration

- Uses SQLite database (axonhub.db) by default.
- The Rust config implementation is canonical in `crates/axonhub-config`; the legacy Go config contract in `conf/conf.go` remains as historical reference.
- Backend API: port 8090, Frontend dev server: port 5173 (proxies to backend).
- Go version: 1.26.0+ (legacy).
- Rust workspace is rooted at `Cargo.toml`.

## Project Overview

AxonHub is an all-in-one AI development platform that serves as a unified API gateway for multiple AI providers. The Rust workspace is the canonical backend implementation. The legacy Go tree remains in-repo as historical reference/oracle material and is not a canonical build, release, or deployment path.

### Core Architecture

- **Transformation Pipeline**: Bidirectional data transformation between clients and AI providers
- **Unified API Layer**: OpenAI/Anthropic-compatible interfaces with automatic translation
- **Channel Management**: Multi-provider support with configurable channels
- **Thread-aware Tracing**: Request tracing with thread linking capabilities
- **Permission System**: RBAC with fine-grained access control
- **System Management**: Web-based configuration interface

## Technology Stack

- **Backend (canonical)**: Rust workspace with Tokio, Actix Web, Serde, and shared workspace dependencies
- **Backend (legacy)**: Go 1.26.0+ with Gin HTTP framework, Ent ORM, gqlgen GraphQL, FX dependency injection
- **Frontend**: React 19 with TypeScript, TanStack Router, TanStack Query, Zustand, Tailwind CSS
- **Database**: SQLite (development), PostgreSQL/MySQL/TiDB (production)
- **Authentication**: JWT with role-based access control

## Backend Structure

### Rust Workspace (Canonical)

- `Cargo.toml` — Root Cargo workspace with shared dependencies
- `apps/axonhub-server` — Rust `axonhub` binary preserving the operator-facing CLI shape
- `crates/axonhub-config` — Rust config loading, defaults, env override, preview/get helpers
- `crates/axonhub-http` — Actix router with `/health` plus the named explicit unsupported boundaries preserved by parity tests

### Legacy Go Backend (Historical Reference)

- `cmd/axonhub/main.go` — Original Go application entry point and CLI contract
- `conf/conf.go` — Original Go configuration loading/defaults contract
- `internal/server/` — HTTP server and route handling with Gin
- `internal/server/biz/` — Core business logic and services
- `internal/server/api/` — REST and GraphQL API handlers
- `internal/server/gql/` — GraphQL schema and resolvers
- `internal/ent/` — Ent ORM for database operations
- `internal/ent/schema/` — Database schema definitions
- `internal/contexts/` — Context handling utilities
- `internal/pkg/` — Shared utilities (xerrors, xjson, xcache, xfile, xcontext, etc.)
- `internal/scopes/` — Permission system with role-based access control
- `llm/` — LLM utilities, transformers, and pipeline processing (separate Go module)
- `llm/pipeline/` — Pipeline processing architecture
- `axon/` — Agent framework with LLM providers, tools, memory (separate Go module)
- `conf/conf.go` — Configuration loading and validation

## Go Modules

- The repository root (`/`) is the main Go module: `github.com/looplj/axonhub`.
- `llm/` is a separate Go module: `github.com/looplj/axonhub/llm`.

### `llm/` Module Notes

- Do not assume root-level Go commands can see packages under `llm/...`.
- When working on files under `llm/`, run Go commands from the `llm/` directory unless you explicitly know a workspace-level command is appropriate.
- Typical examples:
  - `cd llm && go test ./...`
  - `cd llm && go test ./transformer/openai/responses -run TestName`
  - `cd llm && go list ./...`
- If you run `go test ./llm/...` or similar from the repo root, you may hit module boundary errors like `main module does not contain package ...`.
- Apply the same rule to any other nested Go module: use the module root that owns the package you are testing or inspecting.

## Frontend Structure

- `frontend/src/routes/` — TanStack Router file-based routing
- `frontend/src/gql/` — GraphQL API communication
- `frontend/src/features/` — Feature-based component organization
- `frontend/src/components/` — Reusable shared components
- `frontend/src/hooks/` — Custom shared hooks
- `frontend/src/stores/` — Zustand state management
- `frontend/src/locales/` — i18n support (en.json, zh.json)
- `frontend/src/lib/` — Core utilities (API client, i18n, permissions, utils)
- `frontend/src/utils/` — Domain-specific utilities (date, format, error handling)
- `frontend/src/config/` — App configuration
- `frontend/src/context/` — React context providers

## Upstream Sync Notes

When syncing changes from upstream into this fork, follow these repository-specific rules:

1. **Rust remains canonical**
   - Do not treat upstream Go backend changes as the final target implementation.
   - First identify the user-visible behavior changed in legacy Go (`internal/server/**`, `internal/server/biz/**`, `internal/server/gql/**`, `llm/**`), then migrate that behavior into the Rust canonical path:
     - `apps/axonhub-server`
     - `crates/axonhub-http`
     - other Rust workspace crates as needed
   - Only keep Go-side merges as reference/oracle material unless the user explicitly asks to preserve legacy-only behavior there.

2. **Do not restore removed frontend locale files**
   - This fork uses **Paraglide** as the active i18n system.
   - Do **not** reintroduce or keep upstream `frontend/src/locales/**` files when resolving merge conflicts if they were removed in this fork.
   - Treat upstream locale JSON changes as a source of new messages, not as files to restore.

3. **Translate upstream i18n additions into Paraglide**
   - The canonical message source is:
     - `frontend/messages/en.json`
     - `frontend/messages/zh-CN.json`
   - If upstream adds or changes translation keys in legacy locale JSON files, migrate the relevant keys into Paraglide message files using the existing flat key format (for example `apikeys.profiles.title`).
   - Prefer migrating only keys that are actually referenced by merged frontend code or required by the feature being synced; do not blindly bulk-restore the legacy locale tree.

4. **Conflict resolution priority during upstream sync**
   - Preserve this fork's architecture changes first:
     - Rust backend canonicalization
     - Paraglide-based frontend i18n
   - Then port upstream feature additions into those architectures.
   - In practice:
     - keep this fork's Paraglide-based TS/TSX implementation style
    - keep `frontend/src/locales/**` deleted if they were removed here
    - add missing Paraglide message keys for upstream-added UI copy
    - migrate upstream API/backend semantics into Rust endpoints/services instead of stopping at legacy Go merges

5. **Mandatory i18n regression scan after every upstream merge**
   - After resolving merge conflicts (and before finishing the sync), scan for accidental reintroduction of i18next/react-i18next APIs in active frontend code.
   - At minimum, search `frontend/src/**` for these patterns:
     - imports: `react-i18next`, `i18next`
     - APIs/symbols: `useTranslation`, `<Trans`, `I18nextProvider`, `initReactI18next`
   - Treat any hit in active app code as a merge regression unless the user explicitly requested i18next usage.
   - Required remediation: replace i18next usage with Paraglide (`import * as m from '@/paraglide/messages'` and `m["key"]()` / dynamicTranslation helpers) and keep dependencies Paraglide-only.

## Rules Index

All detailed rules are in `.agent/rules/`:

| File | Scope | Description |
|------|-------|-------------|
| [backend.md](.agent/rules/backend.md) | `**/*.{go,rs}` | Rust migration workspace, legacy Go backend, compatibility rules |
| [frontend.md](.agent/rules/frontend.md) | `frontend/**/*.ts,tsx` | React, i18n, UI components, dev commands |
| [e2e.md](.agent/rules/e2e.md) | `frontend/tests/**/*.ts` | E2E testing rules |
| [docs.md](.agent/rules/docs.md) | `docs/**/*.md` | Documentation rules |
| [workflows/add-channel.md](.agent/rules/workflows/add-channel.md) | Manual | Workflow for adding a new channel |
