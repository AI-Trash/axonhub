# 开发指南

## 迁移状态

AxonHub 当前处于增量式的 Go → Rust 后端迁移阶段。

- **当前完整后端：** `cmd/axonhub/main.go`、`conf/conf.go`、`internal/server/` 下的 Go 实现
- **Rust 迁移切片：** 以 `Cargo.toml` 为根的 Cargo workspace
- **Rust 当前已实现：** 配置加载、CLI 兼容、`/health`、`GET /admin/system/status`，以及未迁移 HTTP 路由族的显式 `501 Not Implemented` 返回
- **尚未迁移：** GraphQL、Ent 数据访问、认证流程、provider 编排，以及完整 API 对等能力

如果你需要完整产品能力，请继续使用 Go 后端或当前发布的 Docker / 二进制版本。如果你在做迁移工作，请使用 Rust workspace。

## 架构概览

AxonHub 仍然是一个统一的 AI 网关，核心仍是客户端 SDK 与上游模型提供商之间的双向请求/响应转换链路。

<div align="center">
  <img src="../../transformation-flow.svg" alt="AxonHub Transformation Flow" width="900"/>
</div>

这次迁移改变的是实现路径，不是产品目标：

- 先保留现有对外契约，
- 再按功能切片逐步迁移，
- 对未迁移能力明确返回，而不是伪装成已完成。

## 技术栈

### 后端

- **稳定实现：** Go 1.26+、Gin、Ent、gqlgen、FX
- **迁移切片：** Rust 1.78+、Tokio、Axum、Serde、Cargo workspace + workspace 依赖

### 前端

- React 19
- TypeScript
- Tailwind CSS
- TanStack Router
- Zustand

## 前置要求

- Rust 1.78+
- 如果需要修改旧后端代码，则需要 Go 1.26+
- Node.js 18+ 与 pnpm
- Git

## 仓库结构

### Rust Workspace

- `Cargo.toml` — workspace 根与共享依赖版本
- `apps/axonhub-server` — Rust `axonhub` 二进制入口
- `crates/axonhub-config` — 配置契约、默认值、环境变量覆盖、preview/get 帮助函数
- `crates/axonhub-http` — 提供 `/health`、`GET /admin/system/status` 与显式 `501` 路由桩的 Axum Router

### 旧 Go 后端

- `cmd/axonhub/main.go` — 当前 CLI / 服务契约
- `conf/conf.go` — 配置默认值与兼容契约
- `internal/server/` — 当前完整 HTTP 能力
- `internal/server/gql/` — GraphQL schema 与 resolver
- `internal/ent/` — Ent 模型、schema 与迁移

## Rust 迁移切片开发方式

在仓库根目录运行 Rust CLI：

```bash
cargo run -p axonhub-server -- help
cargo run -p axonhub-server -- config preview
cargo run -p axonhub-server -- config validate
cargo run -p axonhub-server -- config get server.port
cargo run -p axonhub-server --
```

当前 Rust 行为是刻意受限且真实的：

- `/health` 返回真实健康状态
- `/admin/system/status` 在受支持的 SQLite 路径下可通过 Rust 切片返回旧后端的初始化布尔值
- `/admin/*`、`/v1/*`、`/anthropic/v1/*`、`/gemini/*`、`/openapi/*` 等未迁移路由族会返回结构化 `501 Not Implemented` JSON
- 配置文件路径与 `AXONHUB_*` 环境变量命名对齐 `conf/conf.go` 的首个共享契约

## 前端开发

```bash
cd frontend
pnpm install
pnpm dev
```

前端开发服务器运行在 `http://localhost:5173`，并代理到你当前使用的本地后端。

## 旧 Go 工作流

如果你需要当前生产能力，请继续在旧 Go 后端上开发。

典型场景包括：

- GraphQL 与管理后台流程
- Ent schema 与数据库服务
- JWT / API Key 认证与中间件
- provider 编排与 outbound transformer

只有在修改这些旧区域时，下面的 Go 命令才仍然适用：

```bash
go test ./...
make generate
```

当你修改 Ent 或 GraphQL schema 输入时，需要使用 `make generate` 维护旧代码生成产物。

## 验证

推荐的验证命令取决于你修改的是哪一套后端：

### Rust 切片

```bash
cargo check --workspace
cargo test --workspace
```

### 旧 Go 后端

```bash
go test ./...
```

### 前端

```bash
cd frontend
pnpm build
```

## 添加新的 Channel

在 provider 路由迁移完成之前，新渠道仍然需要通过旧 Go 后端和前端配置共同完成。

1. **在 Ent Schema 中扩展渠道枚举**
   - 在 [internal/ent/schema/channel.go](../../../internal/ent/schema/channel.go) 的 `field.Enum("type")` 中增加 provider 标识
   - 在需要时重新生成旧产物

2. **在 Go 中接入 outbound transformer**
   - 更新 `ChannelService.buildChannel`
   - 在旧后端内补充对应 transformer 实现

3. **在前端注册 Provider 元数据**
   - 更新 [frontend/src/features/channels/data/config_providers.ts](../../../frontend/src/features/channels/data/config_providers.ts)
   - 保持 `CHANNEL_CONFIGS` 与相关 helper 一致

4. **同步前端 schema 与展示**
   - 更新 [frontend/src/features/channels/data/schema.ts](../../../frontend/src/features/channels/data/schema.ts)
   - 更新 [frontend/src/features/channels/data/constants.ts](../../../frontend/src/features/channels/data/constants.ts)

5. **补充国际化文案**
   - 更新 [frontend/src/locales/en.json](../../../frontend/src/locales/en.json)
   - 更新 [frontend/src/locales/zh.json](../../../frontend/src/locales/zh.json)
