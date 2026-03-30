# Configuration Guide

## Backend Contract Context

AxonHub's canonical backend implementation is Rust. The legacy Go tree remains in-repo as historical reference/oracle material rather than the current/full product runtime.

This document describes the operator-facing configuration contract preserved by the Rust backend. The current Rust implementation supports config loading, preview, validation, key lookup, and the verified backend surface documented in the maintained repository guidance. Accepted explicit unsupported boundaries remain explicit `501 Not Implemented` responses; this document does **not** imply that every route family is currently supported.

## Overview

AxonHub supports YAML configuration files plus environment variable overrides.

For the Rust backend, configuration is loaded from these paths in order of discovery:

1. `./config.yml`
2. `/etc/axonhub/config.yml`
3. `$HOME/.config/axonhub/config.yml`
4. `./conf/config.yml`

Environment variables use the `AXONHUB_` prefix and preserve the existing dotted-key naming contract by replacing dots with underscores.

## YAML Configuration Example

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

## Environment Variable Example

```bash
export AXONHUB_SERVER_PORT=8090
export AXONHUB_DB_DIALECT="sqlite3"
export AXONHUB_DB_DSN="file:axonhub.db?cache=shared&_fk=1&_pragma=journal_mode(WAL)"
export AXONHUB_LOG_LEVEL="info"
```

## Shared Configuration Reference

### Server

```yaml
server:
  host: "0.0.0.0"
  port: 8090
  name: "AxonHub"
  base_path: ""
  request_timeout: "30s"
  llm_request_timeout: "600s"
  trace:
    thread_header: "AH-Thread-Id"
    trace_header: "AH-Trace-Id"
    extra_trace_headers: []
    extra_trace_body_fields: []
    claude_code_trace_enabled: false
    codex_trace_enabled: false
  debug: false
  disable_ssl_verify: false
```

Common environment variables:

- `AXONHUB_SERVER_HOST`
- `AXONHUB_SERVER_PORT`
- `AXONHUB_SERVER_NAME`
- `AXONHUB_SERVER_BASE_PATH`
- `AXONHUB_SERVER_REQUEST_TIMEOUT`
- `AXONHUB_SERVER_LLM_REQUEST_TIMEOUT`
- `AXONHUB_SERVER_TRACE_THREAD_HEADER`
- `AXONHUB_SERVER_TRACE_TRACE_HEADER`
- `AXONHUB_SERVER_TRACE_EXTRA_TRACE_HEADERS`
- `AXONHUB_SERVER_TRACE_EXTRA_TRACE_BODY_FIELDS`
- `AXONHUB_SERVER_TRACE_CLAUDE_CODE_TRACE_ENABLED`
- `AXONHUB_SERVER_TRACE_CODEX_TRACE_ENABLED`
- `AXONHUB_SERVER_DEBUG`
- `AXONHUB_SERVER_DISABLE_SSL_VERIFY`

### CORS

```yaml
server:
  cors:
    enabled: false
    debug: false
    allowed_origins:
      - "http://localhost:8090"
    allowed_methods: ["GET", "POST", "DELETE", "PATCH", "PUT", "OPTIONS", "HEAD"]
    allowed_headers: ["Content-Type", "Authorization", "X-API-Key", "X-Goog-Api-Key", "X-Project-ID", "X-Thread-ID", "X-Trace-ID"]
    exposed_headers: []
    allow_credentials: false
    max_age: "30m"
```

### API Auth

```yaml
server:
  api:
    auth:
      allow_no_auth: false
```

Environment variable:

- `AXONHUB_SERVER_API_AUTH_ALLOW_NO_AUTH`

### Database

```yaml
db:
  dialect: "sqlite3"
  dsn: "file:axonhub.db?cache=shared&_fk=1&_pragma=journal_mode(WAL)"
  debug: false
```

Environment variables:

- `AXONHUB_DB_DIALECT`
- `AXONHUB_DB_DSN`
- `AXONHUB_DB_DEBUG`

### Log

```yaml
log:
  name: "axonhub"
  debug: false
  skip_level: 1
  level: "info"
  level_key: "level"
  time_key: "time"
  caller_key: "label"
  function_key: ""
  name_key: "logger"
  encoding: "json"
  includes: []
  excludes: []
  output: "stdio"
  file:
    path: "logs/axonhub.log"
    max_size: 100
    max_age: 30
    max_backups: 10
    local_time: true
```

### Metrics

```yaml
metrics:
  enabled: false
  exporter:
    type: ""
    endpoint: ""
    insecure: false
```

### GC

```yaml
gc:
  cron: "0 2 * * *"
  vacuum_enabled: false
  vacuum_full: false
```

### Cache

```yaml
cache:
  mode: "memory"
  memory:
    expiration: "5m"
    cleanup_interval: "10m"
  redis:
    addr: ""
    url: ""
    username: ""
    password: ""
    db: null
    tls: false
    tls_insecure_skip_verify: false
    expiration: ""
```

The Rust backend also normalizes legacy cache keys:

- `cache.default_expiration` → `cache.memory.expiration`
- `cache.cleanup_interval` → `cache.memory.cleanup_interval`

### Provider Quota

```yaml
provider_quota:
  check_interval: "20m"
```

Environment variable:

- `AXONHUB_PROVIDER_QUOTA_CHECK_INTERVAL`

## Validation

The Rust CLI preserves the top-level config verbs from the Go CLI:

```bash
cargo run -p axonhub-server -- config preview
cargo run -p axonhub-server -- config preview --format json
cargo run -p axonhub-server -- config validate
cargo run -p axonhub-server -- config get server.port
```

The current validation checks the same minimum operator-facing rules as the preserved operator-facing AxonHub CLI contract:

- `server.port` must be between `1` and `65535`
- `db.dsn` must not be empty
- `log.name` must not be empty
- `server.cors.allowed_origins` must not be empty when CORS is enabled
- configured log levels and duration strings must be parseable

## Notes About the Current Contract

- This document reflects the Rust canonical backend's current configuration contract.
- A valid Rust config does **not** mean every route family outside the verified surface is currently supported.
- Accepted explicit unsupported boundaries still return structured `501 Not Implemented` responses instead of partial behavior.
- The legacy Go tree remains available in-repo only as historical reference/oracle material.

## Related Documentation

- [Docker Deployment](docker.md)
- [Quick Start Guide](../getting-started/quick-start.md)
- [Development Guide](../development/development.md)
