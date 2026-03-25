# 开发指南

## 迁移状态

AxonHub 当前处于增量式的 Go → Rust 后端迁移阶段。

- **当前完整后端：** `cmd/axonhub/main.go`、`conf/conf.go`、`internal/server/` 下的 Go 实现
- **Rust 迁移切片：** 以 `Cargo.toml` 为根的 Cargo workspace

### Rust 切片当前覆盖范围（已验证）

Rust 切片实现了以下已验证的 SQLite 和 PostgreSQL 能力面：

- **配置与 CLI**: 配置加载、CLI 兼容（`config preview`、`config validate`、`config get`、`version`、`help`）
- **健康与系统**: `/health`、`GET /admin/system/status`、`POST /admin/system/initialize`
- **身份与上下文**: 认证、请求上下文、JWT 处理
- **管理只读路由**: `GET /admin/requests/:request_id/content`
- **管理 GraphQL**: `POST /admin/graphql` 含 playground，OAuth 流程（Codex、Claude Code、Antigravity、Copilot）
- **OpenAPI GraphQL**: `POST /openapi/v1/graphql` 含 playground
- **OpenAI 兼容 `/v1` 推理**: `/models`、`/chat/completions`、`/responses`、`/embeddings`、`/messages`、`/rerank`
- **视频生成**: `POST /v1/videos`、`GET /v1/videos/{id}`、`DELETE /v1/videos/{id}`
- **其他提供商 API**: Jina、Anthropic、Gemini、Doubao 等路由（见 routes 文件）
- **数据库支持**: SQLite 和 PostgreSQL 完全验证；MySQL 通过共享 SeaORM 接缝已布线但完整集成验证待完成

### Rust 切片剩余工作（明确分桶）

**Next（高优先级、近期）:**

- 图像生成端点（`/v1/images/generations`、`/v1/images/edits`）
- RBAC/权限系统迁移（internal/scopes）
- 核心业务逻辑表面（internal/server/biz）：渠道/模型管理、请求生命周期、用量/成本、追踪/线程
- Transformer/Pipeline 迁移（llm/transformer, llm/pipeline）：提供商编排、出站适配器
- 模型关联/获取对等
- 系统初始化完整性（引导流程、默认数据）
- OAuth 对等验证（完整 OAuth 流程实现）
- MySQL 集成验证完成

**Later（中优先级、中期）:**

- AiSDK 兼容性（完整 Vercel AI SDK 协议）
- 完整管理 GraphQL 写操作和高级查询
- 高级/企业级功能：提示词保护、提供商配额管理、熔断器、渠道自动禁用
- 配置对齐：与旧 Go 后端配置选项的完全对等
- 更广泛的测试对等：匹配 Go 套件的集成测试覆盖
- 更多提供商特定功能（Gemini 工具、Anthropic 扩展）

**Deferred with 501（明确边界）:**

- 仍为 Go 独有的运营/后台项目，直到单独迁移闸门
- 遗留独有数据库方言（TiDB、Neon DB）— 保持在 Go 后端
- Helm Kubernetes 部署路径（仅 Go 后端）
- 不属于目标 Rust 切片的完整旧 Go API 表面

### 未移植 HTTP 族（真实 501）

超出已验证范围的路由族会返回结构化 `501 Not Implemented` JSON：

- `/v1/images/generations`、`/v1/images/edits`（图像生成）
- `/admin/*` 写操作（用户管理、项目创建、角色分配等）
- 未纳入目标的提供商包装器
- 实时 API 端点
- 超越只读操作的完整管理后台

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
- **迁移切片：** Rust 1.78+、Tokio、Actix Web、Serde、Cargo workspace + shared dependencies

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
- `crates/axonhub-http` — Actix 路由器，提供 `/health`、已验证的 SQLite 与 PostgreSQL 支撑的 bootstrap/system 路由、已迁移的 OpenAI 兼容 `/v1` 路由，以及面向未迁移路由族的显式 `501` 路由桩

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
- `/admin/system/status` 与 `/admin/system/initialize` 在受支持的 SQLite 与 PostgreSQL 迁移路径下可用
- `/v1/models`、`/v1/chat/completions`、`/v1/responses`、`/v1/embeddings` 通过已迁移的 Rust 实用切片执行，并带有 auth/context、路由语义与 SQLite / PostgreSQL 持久化副作用
- MySQL 通过同一套 SeaORM repository seam 已完成布线，但 Rust 侧完整集成验证仍待完成；TiDB 与 Neon DB 仍保留在 Go 后端
- `/admin/*` 写操作、未纳入目标的 `/v1/*` 路由（如图像生成）以及其他仍未迁移的路由族，都会继续作为显式边界返回结构化 `501 Not Implemented` JSON
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
