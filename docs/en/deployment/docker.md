# Docker Deployment

## Important Migration Note

The repository now contains a Rust backend workspace, but the Docker deployment path should still be understood as the **full product deployment path**, not as proof that the Rust backend already has full feature parity.

Today:

- the **legacy Go backend** remains the complete runtime used for full AxonHub behavior,
- the **Rust backend** is an in-repo migration slice focused on config, CLI compatibility, `/health`, SQLite-scoped bootstrap/system routes, the migrated OpenAI-compatible practical `/v1` subset, and explicit `501` responses for unported API families.
- the default `looplj/axonhub:latest` image still follows the full-product path, while `looplj/axonhub:rust-latest` publishes the current Rust migration slice separately.

Use `looplj/axonhub:latest` when you want the current full AxonHub deployment experience. Use the Rust workspace or `looplj/axonhub:rust-latest` when working with the migration slice.

## Overview

This guide covers Docker and Docker Compose deployment for the current full AxonHub runtime.

## Rust Migration-Slice Docker Path

The repository now also publishes a dedicated Rust migration-slice image:

- image tags: `looplj/axonhub:rust-latest` and `looplj/axonhub:rust-<tag>`
- compose example: `docker compose -f docker-compose.rust.yml up -d`

Example:

```bash
docker run --rm -p 8090:8090 looplj/axonhub:rust-latest
```

The compose example keeps the Rust slice's default SQLite data and other relative runtime files on a named Docker volume. A one-off `docker run --rm` container is intentionally ephemeral unless you add your own volume mounts.

What to expect from that image today:

- `/health` is available
- `GET /admin/system/status` and `POST /admin/system/initialize` are only intended for the compatible SQLite-backed migration path
- the migrated OpenAI-compatible practical `/v1` subset (`/v1/models`, `/v1/chat/completions`, `/v1/responses`, `/v1/embeddings`) is available when the compatible SQLite migration data path is configured
- unported route families return explicit `501 Not Implemented` JSON
- it is not a full-product replacement yet

## Quick Start

### 1. Clone the Repository

```bash
git clone https://github.com/looplj/axonhub.git
cd axonhub
```

### 2. Configure Environment

```bash
cp config.example.yml config.yml
```

Edit `config.yml`:

```yaml
server:
  port: 8090
  name: "AxonHub"

db:
  dialect: "sqlite3"
  dsn: "file:axonhub.db?cache=shared&_fk=1&_pragma=journal_mode(WAL)"

log:
  level: "info"
  encoding: "json"
```

### 3. Start Services

```bash
docker-compose up -d
```

### 4. Verify Deployment

```bash
docker-compose ps
```

Access the application at `http://localhost:8090`.

## Example Docker Compose

```yaml
version: '3.8'

services:
  axonhub:
    image: looplj/axonhub:latest
    ports:
      - "8090:8090"
    volumes:
      - ./config.yml:/app/config.yml:ro
    environment:
      - AXONHUB_SERVER_PORT=8090
      - AXONHUB_DB_DIALECT=sqlite3
      - AXONHUB_DB_DSN=file:axonhub.db?cache=shared&_fk=1&_pragma=journal_mode(WAL)
    restart: unless-stopped
```

## Health Checks

The current deployment path exposes the standard health endpoint:

```yaml
healthcheck:
  test: ["CMD", "curl", "-f", "http://localhost:8090/health"]
  interval: 30s
  timeout: 10s
  retries: 3
  start_period: 40s
```

## Runtime Expectations During Migration

Keep these distinctions clear while the migration is in progress:

- Docker deployment is still about the **current full backend experience**.
- The Rust workspace and `looplj/axonhub:rust-*` images are currently a **developer migration slice**, not the production-complete replacement.
- If you run the Rust backend directly from the workspace, `/health`, the SQLite-scoped bootstrap/system routes, and the migrated OpenAI-compatible practical `/v1` subset are available alongside explicit `501` route stubs for every remaining family.

## Troubleshooting

### Container fails to start

- Check Docker logs: `docker-compose logs axonhub`
- Verify configuration file permissions
- Confirm database connectivity

### Port conflicts

- Change the published port in `docker-compose.yml`
- Check whether another service already uses port `8090`

### Database connection issues

- Verify credentials and DSN
- Confirm network connectivity between containers
- Ensure the database container is healthy

## Related Documentation

- [Configuration Guide](configuration.md)
- [Quick Start Guide](../getting-started/quick-start.md)
- [Development Guide](../development/development.md)
