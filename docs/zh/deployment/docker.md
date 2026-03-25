# Docker 部署

## 重要迁移说明

当前仓库已经进入受支持 SQLite 与 PostgreSQL AxonHub 能力面的 Rust 最终切换阶段。

当前状态是：

- **Rust 后端** 已经是仓库内已验证 SQLite 与 PostgreSQL 替代范围的受支持运行时：CLI/config、`/health`、admin bootstrap/status/auth/read 路由、admin GraphQL、OpenAPI GraphQL、request-context/auth 基础能力，以及仓库证据已覆盖的 inference 路由族；
- 超出该已验证范围的路由族，仍然会由 Rust 返回显式 `501 Not Implemented`；
- `looplj/axonhub:latest` 只在 Go 退役闸门关闭前作为回滚目标保留。

对于当前受支持的运行时，请使用 Rust workspace、`ghcr.io/looplj/axonhub:rust-latest` 或 `docker-compose.rust.yml`。只有在做回滚演练或恢复验证时，才保留 `looplj/axonhub:latest`。

## 概述

本文档说明当前仓库受支持 Rust 运行时的 Docker 与 Docker Compose 部署方式。

## Rust 切换 Docker 路径

仓库现在会发布面向当前受支持替代范围的专用 Rust 镜像：

- 镜像标签：`ghcr.io/looplj/axonhub:rust-latest` 与 `ghcr.io/looplj/axonhub:rust-<tag>`
- Compose 示例：`docker compose -f docker-compose.rust.yml up -d`

示例：

```bash
docker run --rm -p 8090:8090 ghcr.io/looplj/axonhub:rust-latest
```

Compose 示例会把 Rust 运行时默认的 SQLite 数据以及其他相对路径运行时文件保存在一个具名 Docker volume 中。一次性的 `docker run --rm` 容器则是刻意保持临时性的，除非你自行挂载持久化卷。

这些 Rust 标记交付路径对应的二元 PASS/FAIL 切换、HOLD 与 ROLLBACK 条件，统一定义在 `.sisyphus/artifacts/rust-backend-seaorm-actix-migration-plan/final-cutover-gates.md`。

当前这个镜像的行为预期是：

- `/health` 可用
- `GET /admin/system/status` 与 `POST /admin/system/initialize` 在已验证的 SQLite 与 PostgreSQL 运行时路径上受支持
- 已验证的 Rust 替代范围还包括 admin auth/read 流程、admin GraphQL、OpenAPI GraphQL、request-context/auth 基础能力，以及已迁移的 inference 路由族，并适用于已接受的 SQLite 与 PostgreSQL 路径
- 超出该已验证范围的路由族，会返回显式 `501 Not Implemented` JSON，而不是再引导到旧 Go 后端
- MySQL 已通过同一套 SeaORM repository seam 完成布线，但 Rust 测试套件中尚未完成完整集成验证
- SQLite 与 PostgreSQL 之外的多方言替代能力仍不属于当前切换闸门范围

## 快速开始

### 1. 克隆仓库

```bash
git clone https://github.com/looplj/axonhub.git
cd axonhub
```

### 2. 配置环境

```bash
cp config.example.yml config.yml
```

编辑 `config.yml`：

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

### 3. 启动服务

```bash
docker compose -f docker-compose.rust.yml up -d
```

### 4. 验证部署

```bash
docker compose -f docker-compose.rust.yml ps
```

访问 `http://localhost:8090`。

## Docker Compose 示例

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

## 健康检查

当前部署路径仍然暴露标准健康检查接口：

```yaml
healthcheck:
  test: ["CMD", "curl", "-f", "http://localhost:8090/health"]
  interval: 30s
  timeout: 10s
  retries: 3
  start_period: 40s
```

## 切换期间的运行时预期

在 Go 退役闸门仍在收尾期间，请明确区分下面几件事：

- 对于当前受支持范围，Docker 部署现在以 Rust 运行时为中心；
- 已验证的替代范围当前覆盖 **SQLite 与 PostgreSQL**；MySQL 已布线但尚未完成完整集成验证，未支持方言与未迁移路由族仍然是明确排除项；
- 如果你直接运行 Rust backend，目前可用的是 `/health`、基于 SQLite/PostgreSQL 的 admin/bootstrap/auth/GraphQL 能力面、已迁移的 inference 路由族，以及其余未支持路由族的显式 `501` 路由桩。

## 故障排查

### 容器启动失败

- 查看 Docker 日志：`docker-compose logs axonhub`
- 检查配置文件权限
- 确认数据库连接可用

### 端口冲突

- 修改 `docker-compose.yml` 中映射的端口
- 检查是否已有其他服务占用 `8090`

### 数据库连接问题

- 检查凭据与 DSN
- 确认容器之间网络连通
- 确认数据库容器健康状态正常

## 相关文档

- [配置指南](configuration.md)
- [快速入门](../getting-started/quick-start.md)
- [开发指南](../development/development.md)
