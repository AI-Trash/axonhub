# Docker 部署

## 重要迁移说明

仓库中现在已经包含 Rust 后端 workspace，但 Docker 部署路径目前仍应理解为**完整产品部署路径**，而不是 Rust 后端已经具备完整能力的证明。

当前状态是：

- **旧 Go 后端** 仍然承担完整的 AxonHub 运行时能力；
- **Rust 后端** 只是仓库内的迁移切片，当前重点是配置、CLI 兼容、`/health`、SQLite 范围内的 bootstrap/system 路由、已迁移的 OpenAI 兼容实用 `/v1` 子集，以及未迁移 API 路由族的显式 `501` 返回。
- 默认的 `looplj/axonhub:latest` 镜像仍然对应完整产品路径，而 `ghcr.io/looplj/axonhub:rust-latest` 会单独发布当前 Rust 迁移切片。

如果你需要完整的 AxonHub 部署体验，请继续使用 `looplj/axonhub:latest`。若你在做迁移开发，请使用 Rust workspace 或 `ghcr.io/looplj/axonhub:rust-latest`。

## 概述

本文档说明当前完整 AxonHub 运行时的 Docker 与 Docker Compose 部署方式。

## Rust 迁移切片 Docker 路径

仓库现在还会额外发布专门的 Rust 迁移切片镜像：

- 镜像标签：`ghcr.io/looplj/axonhub:rust-latest` 与 `ghcr.io/looplj/axonhub:rust-<tag>`
- Compose 示例：`docker compose -f docker-compose.rust.yml up -d`

示例：

```bash
docker run --rm -p 8090:8090 ghcr.io/looplj/axonhub:rust-latest
```

Compose 示例会把 Rust 切片默认的 SQLite 数据以及其他相对路径运行时文件保存在一个具名 Docker volume 中。一次性的 `docker run --rm` 容器则是刻意保持临时性的，除非你自行挂载持久化卷。

当前这个镜像的行为预期是：

- `/health` 可用
- `GET /admin/system/status` 与 `POST /admin/system/initialize` 只面向兼容的 SQLite 迁移路径
- 已迁移的 OpenAI 兼容实用 `/v1` 子集（`/v1/models`、`/v1/chat/completions`、`/v1/responses`、`/v1/embeddings`）会在兼容的 SQLite 迁移数据路径准备好时可用
- 未迁移路由族返回显式 `501 Not Implemented` JSON
- 它还不是完整产品的替代品

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
docker-compose up -d
```

### 4. 验证部署

```bash
docker-compose ps
```

访问 `http://localhost:8090`。

## Docker Compose 示例

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

## 迁移期间的运行时预期

在迁移过程中，请明确区分下面几件事：

- Docker 部署仍然对应**当前完整后端能力**；
- Rust workspace 与 `ghcr.io/looplj/axonhub:rust-*` 镜像目前只是**开发中的迁移切片**，不是生产可完全替代的后端；
- 如果你直接运行 Rust backend，目前可用的是 `/health`、SQLite 范围内的 bootstrap/system 路由、已迁移的 OpenAI 兼容实用 `/v1` 子集，以及其余路由族的显式 `501` 路由桩。

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
