# Docker Deployment

## Important Migration Note

The repository now contains a Rust backend workspace, but the Docker deployment path should still be understood as the **full product deployment path**, not as proof that the Rust backend already has full feature parity.

Today:

- the **legacy Go backend** remains the complete runtime used for full AxonHub behavior,
- the **Rust backend** is an in-repo migration slice focused on config, CLI compatibility, `/health`, `GET /admin/system/status`, and explicit `501` responses for unported API families.

Use Docker when you want the current full AxonHub deployment experience. Use the Rust workspace separately when developing the migration.

## Overview

This guide covers Docker and Docker Compose deployment for the current full AxonHub runtime.

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
- The Rust workspace is currently a **developer migration slice**, not the production-complete replacement.
- If you run the Rust backend directly from the workspace, `/health` and `GET /admin/system/status` are currently available alongside explicit `501` route stubs.

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
