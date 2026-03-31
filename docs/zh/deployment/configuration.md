# 配置指南

## 当前契约背景

AxonHub 在此仓库中的 canonical 后端实现已经是 Rust。

- **Rust backend**：当前对外的 canonical 配置契约与受支持运行时路径
- **旧 Go 树**：保留在仓库中作为历史参考 / oracle 材料，不再是当前仓库的 canonical 部署契约

本文描述的是当前 Rust backend 对外保留的配置契约。配置加载、`config preview`、`config validate`、`config get` 与当前受支持的 Rust 运行时，都遵循这里说明的配置形状。

## 概述

AxonHub 支持 YAML 配置文件和环境变量覆盖。

对于当前 Rust backend，配置文件会按以下发现顺序加载：

1. `./config.yml`
2. `/etc/axonhub/config.yml`
3. `$HOME/.config/axonhub/config.yml`
4. `./conf/config.yml`

环境变量统一使用 `AXONHUB_` 前缀，并沿用现有的点号转下划线命名契约。

## YAML 配置示例

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

## 环境变量示例

```bash
export AXONHUB_SERVER_PORT=8090
export AXONHUB_DB_DIALECT="sqlite3"
export AXONHUB_DB_DSN="file:axonhub.db?cache=shared&_fk=1&_pragma=journal_mode(WAL)"
export AXONHUB_LOG_LEVEL="info"
```

## 共享配置参考

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

常见环境变量：

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

环境变量：

- `AXONHUB_SERVER_API_AUTH_ALLOW_NO_AUTH`

### Database

```yaml
db:
  dialect: "sqlite3"
  dsn: "file:axonhub.db?cache=shared&_fk=1&_pragma=journal_mode(WAL)"
  debug: false
```

环境变量：

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

当前 Rust 配置契约也兼容旧缓存字段：

- `cache.default_expiration` → `cache.memory.expiration`
- `cache.cleanup_interval` → `cache.memory.cleanup_interval`

### Provider Quota

```yaml
provider_quota:
  check_interval: "20m"
```

环境变量：

- `AXONHUB_PROVIDER_QUOTA_CHECK_INTERVAL`

## 验证

Rust CLI 保留了 Go CLI 的顶层配置命令形状：

```bash
cargo run -p axonhub-server -- config preview
cargo run -p axonhub-server -- config preview --format json
cargo run -p axonhub-server -- config validate
cargo run -p axonhub-server -- config get server.port
```

当前验证切片覆盖了与旧 Go 命令相同的最小运维规则：

- `server.port` 必须在 `1` 到 `65535` 之间
- `db.dsn` 不能为空
- `log.name` 不能为空
- 当启用 CORS 时，`server.cors.allowed_origins` 不能为空
- 配置中的日志级别和 duration 字符串必须可解析

## 关于能力对等的说明

- 当前迁移顺序是先迁移配置契约，再迁移完整运行时行为。
- Rust 配置校验通过，**并不表示** Go 后端的全部能力已经在 Rust 中可用。
- 对于未迁移的运行时接口，Rust 后端会显式返回 `501 Not Implemented`，而不是提供不完整的假实现。

## 相关文档

- [Docker 部署](docker.md)
- [快速入门](../getting-started/quick-start.md)
- [开发指南](../development/development.md)
