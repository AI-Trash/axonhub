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

## Rules Index

All detailed rules are in `.agent/rules/`:

| File | Scope | Description |
|------|-------|-------------|
| [backend.md](.agent/rules/backend.md) | `**/*.{go,rs}` | Rust migration workspace, legacy Go backend, compatibility rules |
| [frontend.md](.agent/rules/frontend.md) | `frontend/**/*.ts,tsx` | React, i18n, UI components, dev commands |
| [e2e.md](.agent/rules/e2e.md) | `frontend/tests/**/*.ts` | E2E testing rules |
| [docs.md](.agent/rules/docs.md) | `docs/**/*.md` | Documentation rules |
| [workflows/add-channel.md](.agent/rules/workflows/add-channel.md) | Manual | Workflow for adding a new channel |
