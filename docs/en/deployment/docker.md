# Docker Deployment

## Important Migration Note

The repository is now in the final Rust cutover stage for the supported SQLite-backed AxonHub surface.

Today:

- the **Rust backend** is the supported runtime for the verified SQLite-backed replacement scope in this repository: CLI/config, `/health`, admin bootstrap/status/auth/read routes, admin GraphQL, OpenAPI GraphQL, request-context/auth foundations, and the migrated inference families already covered by repo evidence,
- route families outside that verified scope still return explicit Rust `501 Not Implemented` payloads,
- `looplj/axonhub:latest` remains available only as the rollback target while the Go retirement gates are being closed.

Use the Rust workspace, `ghcr.io/looplj/axonhub:rust-latest`, or `docker-compose.rust.yml` for the supported runtime. Keep `looplj/axonhub:latest` only for rollback rehearsal or recovery validation.

## Overview

This guide covers Docker and Docker Compose deployment for the supported Rust runtime in this repository.

## Rust Cutover Docker Path

The repository publishes a dedicated Rust image for the supported replacement scope:

- image tags: `ghcr.io/looplj/axonhub:rust-latest` and `ghcr.io/looplj/axonhub:rust-<tag>`
- compose example: `docker compose -f docker-compose.rust.yml up -d`

Example:

```bash
docker run --rm -p 8090:8090 ghcr.io/looplj/axonhub:rust-latest
```

The compose example keeps the Rust runtime's default SQLite data and other relative runtime files on a named Docker volume. A one-off `docker run --rm` container is intentionally ephemeral unless you add your own volume mounts.

What to expect from that image today:

- `/health` is available
- `GET /admin/system/status` and `POST /admin/system/initialize` are supported on the verified SQLite-backed runtime path
- the verified Rust replacement scope also covers admin auth/read flows, admin GraphQL, OpenAPI GraphQL, request-context/auth foundations, and the migrated inference families when the compatible SQLite data path is configured
- route families outside that verified scope return explicit `501 Not Implemented` JSON instead of redirecting operators to the legacy Go backend
- multi-dialect replacement beyond SQLite is still out of scope for this cutover gate

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
docker compose -f docker-compose.rust.yml up -d
```

### 4. Verify Deployment

```bash
docker compose -f docker-compose.rust.yml ps
```

Access the application at `http://localhost:8090`.

## Example Docker Compose

```yaml
version: '3.8'

services:
  axonhub:
    image: ghcr.io/looplj/axonhub:rust-latest
    ports:
      - "8090:8090"
    volumes:
      - ./config.yml:/app/config.yml:ro
      - axonhub-rust-data:/app
    environment:
      - AXONHUB_SERVER_PORT=8090
      - AXONHUB_DB_DIALECT=sqlite3
      - AXONHUB_DB_DSN=file:axonhub.db?cache=shared&_fk=1&_pragma=journal_mode(WAL)
    restart: unless-stopped

volumes:
  axonhub-rust-data:
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

## Runtime Expectations During Cutover

Keep these distinctions clear while the Go retirement gates are still in progress:

- Docker deployment for the supported scope now centers on the Rust runtime.
- The verified replacement scope is still **SQLite-backed only**; unsupported dialects and unported route families remain explicit exclusions.
- If you run the Rust backend directly from the workspace, `/health`, the SQLite-backed admin/bootstrap/auth/GraphQL surface, and the migrated inference families are available alongside explicit `501` route stubs for every remaining unsupported family.

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
