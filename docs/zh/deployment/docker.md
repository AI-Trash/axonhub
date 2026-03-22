# Docker 部署

## 重要迁移说明

仓库中现在已经包含 Rust 后端 workspace，但 Docker 部署路径目前仍应理解为**完整产品部署路径**，而不是 Rust 后端已经具备完整能力的证明。

当前状态是：

- **旧 Go 后端** 仍然承担完整的 AxonHub 运行时能力；
- **Rust 后端** 只是仓库内的迁移切片，当前重点是配置、CLI 兼容、`/health`、`GET /admin/system/status`，以及未迁移 API 路由族的显式 `501` 返回。

如果你需要完整的 AxonHub 部署体验，请继续使用 Docker。若你在做迁移开发，请单独使用 Rust workspace。

## 概述

本文档说明当前完整 AxonHub 运行时的 Docker 与 Docker Compose 部署方式。

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
- Rust workspace 目前只是**开发中的迁移切片**，不是生产可完全替代的后端；
- 如果你直接运行 Rust backend，目前可用的是 `/health`、`GET /admin/system/status` 以及显式 `501` 路由桩。

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
