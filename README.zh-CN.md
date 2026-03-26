<div align="center">

# AxonHub - All-in-one AI 开发平台
### 任意 SDK、任意模型、零代码改动

<a href="https://trendshift.io/repositories/16225" target="_blank"><img src="https://trendshift.io/api/badge/repositories/16225" alt="looplj%2Faxonhub | Trendshift" style="width: 250px; height: 55px;" width="250" height="55"/></a>

</div>

<div align="center">

[![测试状态](https://github.com/looplj/axonhub/actions/workflows/test.yml/badge.svg)](https://github.com/looplj/axonhub/actions/workflows/test.yml)
[![Lint 状态](https://github.com/looplj/axonhub/actions/workflows/lint.yml/badge.svg)](https://github.com/looplj/axonhub/actions/workflows/lint.yml)
[![Rust Workspace](https://img.shields.io/badge/rust-workspace-d19132?logo=rust&logoColor=white)](Cargo.toml)
[![旧 Go 后端](https://img.shields.io/badge/legacy-go%20backend-00ADD8?logo=go&logoColor=white)](cmd/axonhub/main.go)
[![Docker Ready](https://img.shields.io/badge/docker-ready-2496ED?logo=docker&logoColor=white)](https://docker.com)

[English](README.md) | [中文](README.zh-CN.md)

</div>

---

## 🚧 后端迁移状态

当前仓库正处于 **Go → Rust 后端最终切换阶段**。

### 当前 Rust 支持切片（已验证）

Rust workspace 与带 Rust 标记的发布交付物，已经是仓库内已验证 SQLite 和 PostgreSQL 能力面的真实替代后端，包括：

- **CLI/config**: 命令行接口和配置加载
- **健康与系统**: `/health`, `GET /admin/system/status`, `POST /admin/system/initialize`
- **身份与上下文**: 认证、请求上下文、JWT 处理
- **管理只读路由**: `GET /admin/requests/:request_id/content`
- **管理 GraphQL**: `POST /admin/graphql` 含 playground
- **OpenAPI GraphQL**: `POST /openapi/v1/graphql` 含 playground
- **OpenAI 兼容 `/v1` 推理（仅标准 JSON 请求）**: `/models`, `/chat/completions`, `/responses`, `/embeddings`, `/messages`, `/rerank`, `/images/generations`
- **视频生成**: `POST /v1/videos`, `GET /v1/videos/{id}`, `DELETE /v1/videos/{id}`
- **其他提供商 API**: Jina、Anthropic、Gemini、Doubao 等路由（见 routes 文件）
- **该已验证切片的数据库支持**: SQLite 和 PostgreSQL

### 当前已实现但尚未完全验证的部分

- **Provider-edge 管理 OAuth 辅助能力**: Codex、Claude Code、Antigravity、Copilot 的管理侧 OAuth 路由已在 Rust 中布线，并对整组路由具备鉴权/边界覆盖；当前正向自动化证据覆盖的是带安全运行时配置的 Codex start，但各提供商完整端到端验证仍未完成。
- **MySQL 支持**: 同一套 SeaORM 支持切片已通过共享 repository seam 与 capability builder 为 MySQL 布线，但 Rust 测试套件中尚未包含完整的 MySQL 自动化集成验证。

### 当前未支持边界

超出上述已验证范围的路由族，仍然会由 Rust 侧返回显式 `501 Not Implemented`，而不是再指引用户回退到旧 Go 后端。具体包括：

- **图像编辑及其余图像变体**: `POST /v1/images/edits` 与其他尚未迁移的图像路由，仍然落在 Rust 显式 `501 Not Implemented` 边界上
- **实时 API**: Rust 当前没有暴露专门的实时 / WebSocket 路由族；代表性的 `/v1/realtime` 请求会落在显式 `501 Not Implemented` 边界上
- **完整管理后台**: 写操作、用户/项目/角色管理、配额配置
- **完整 RBAC/权限系统**: 超越基础管理员认证的细粒度访问控制
- **核心业务逻辑表面**: 渠道/模型关联与获取、用量/成本追踪、追踪/线程管理、系统初始化完整性
- **Transformer/Pipeline 表面**: 提供商编排、出站转换器、中间件管道
- **AiSDK 兼容性**: Rust 仍不支持 Vercel AI SDK 协议；带有 `X-Vercel-Ai-Ui-Message-Stream` 或 `X-Vercel-AI-Data-Stream` 标记的 `/v1` 请求会返回显式 `501 Not Implemented`
- **高级/企业级功能**: 提示词保护、提供商配额管理、熔断器、渠道自动禁用
- **配置对齐**: 与旧 Go 后端配置选项的完全对等
- **测试对等**: 覆盖范围匹配 Go 套件的更广泛集成测试

### 迁移路线图（明确分桶）

**Next（高优先级、近期）:**

- 除 `POST /v1/images/generations` 之外的剩余图像路由（尤其是 `/v1/images/edits`）
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

### 实际含义

对于上述**受支持产品能力面**，请使用 Rust 二进制、`ghcr.io/looplj/axonhub:rust-latest` 或 `docker-compose.rust.yml`。`looplj/axonhub:latest` 仅在 Go 退役闸门完成前作为回滚目标保留。

对于**未支持功能**或尚未验证的数据库方言（TiDB、Neon DB），请继续使用 `cmd/axonhub/main.go` 中的旧 Go 后端或标准 `axonhub` 发布交付物。

---

> 注意
>
> 1. 本项目为个人维护，作者不对使用风险作任何保证，请审慎评估。
> 2. 本项目核心范围不包括 2api（订阅转 API）；如有此类需求，建议使用其他专注于 2api 的开源项目。

---

## 📖 项目介绍

### All-in-one AI 开发平台

**AxonHub 是 AI 网关，让你无需改动一行代码即可切换模型供应商。**

无论你使用的是 OpenAI SDK、Anthropic SDK 还是任何 AI SDK，AxonHub 都会透明地将你的请求转换为与任何支持的模型供应商兼容的格式。无需重构，无需更换 SDK——只需更改配置即可。

**它解决了什么问题：**
- 🔒 **供应商锁定** - 从 GPT-4 瞬间切换到 Claude 或 Gemini
- 🔧 **集成复杂性** - 一个 API 格式对接 10+ 供应商
- 📊 **可观测性缺口** - 开箱即用的完整请求追踪
- 💸 **成本控制** - 实时用量追踪和预算管理

<div align="center">
  <img src="docs/axonhub-architecture-light.svg" alt="AxonHub Architecture" width="700"/>
</div>

### 核心特性 Core Features

| 特性 | 你能获得什么 |
|------|-------------|
| 🔄 [**任意 SDK → 任意模型**](docs/zh/api-reference/openai-api.md) | 用 OpenAI SDK 调用 Claude，或用 Anthropic SDK 调用 GPT。零代码改动。 |
| 🔍 [**完整请求追踪**](docs/zh/guides/tracing.md) | 线程级可观测性的完整请求时间线。更快定位问题。 |
| 🔐 [**企业级 RBAC**](docs/zh/guides/permissions.md) | 细粒度访问控制、用量配额和数据隔离。 |
| ⚡ [**智能负载均衡**](docs/zh/guides/load-balance.md) | <100ms 自动故障转移。始终路由到最健康的渠道。 |
| 💰 [**实时成本追踪**](docs/zh/guides/cost-tracking.md) | 每次请求的成本明细。输入、输出、缓存 Token——全部追踪。 |

---

## 📚 文档 | Documentation

### DeepWiki
详细的技术文档、API 参考、架构设计等内容，可以访问 
- [![DeepWiki](https://img.shields.io/badge/DeepWiki-looplj%2Faxonhub-blue.svg?logo=data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACwAAAAyCAYAAAAnWDnqAAAAAXNSR0IArs4c6QAAA05JREFUaEPtmUtyEzEQhtWTQyQLHNak2AB7ZnyXZMEjXMGeK/AIi+QuHrMnbChYY7MIh8g01fJoopFb0uhhEqqcbWTp06/uv1saEDv4O3n3dV60RfP947Mm9/SQc0ICFQgzfc4CYZoTPAswgSJCCUJUnAAoRHOAUOcATwbmVLWdGoH//PB8mnKqScAhsD0kYP3j/Yt5LPQe2KvcXmGvRHcDnpxfL2zOYJ1mFwrryWTz0advv1Ut4CJgf5uhDuDj5eUcAUoahrdY/56ebRWeraTjMt/00Sh3UDtjgHtQNHwcRGOC98BJEAEymycmYcWwOprTgcB6VZ5JK5TAJ+fXGLBm3FDAmn6oPPjR4rKCAoJCal2eAiQp2x0vxTPB3ALO2CRkwmDy5WohzBDwSEFKRwPbknEggCPB/imwrycgxX2NzoMCHhPkDwqYMr9tRcP5qNrMZHkVnOjRMWwLCcr8ohBVb1OMjxLwGCvjTikrsBOiA6fNyCrm8V1rP93iVPpwaE+gO0SsWmPiXB+jikdf6SizrT5qKasx5j8ABbHpFTx+vFXp9EnYQmLx02h1QTTrl6eDqxLnGjporxl3NL3agEvXdT0WmEost648sQOYAeJS9Q7bfUVoMGnjo4AZdUMQku50McDcMWcBPvr0SzbTAFDfvJqwLzgxwATnCgnp4wDl6Aa+Ax283gghmj+vj7feE2KBBRMW3FzOpLOADl0Isb5587h/U4gGvkt5v60Z1VLG8BhYjbzRwyQZemwAd6cCR5/XFWLYZRIMpX39AR0tjaGGiGzLVyhse5C9RKC6ai42ppWPKiBagOvaYk8lO7DajerabOZP46Lby5wKjw1HCRx7p9sVMOWGzb/vA1hwiWc6jm3MvQDTogQkiqIhJV0nBQBTU+3okKCFDy9WwferkHjtxib7t3xIUQtHxnIwtx4mpg26/HfwVNVDb4oI9RHmx5WGelRVlrtiw43zboCLaxv46AZeB3IlTkwouebTr1y2NjSpHz68WNFjHvupy3q8TFn3Hos2IAk4Ju5dCo8B3wP7VPr/FGaKiG+T+v+TQqIrOqMTL1VdWV1DdmcbO8KXBz6esmYWYKPwDL5b5FA1a0hwapHiom0r/cKaoqr+27/XcrS5UwSMbQAAAABJRU5ErkJggg==)](https://deepwiki.com/looplj/axonhub)
- [![zread](https://img.shields.io/badge/Ask_Zread-_.svg?style=flat&color=00b0aa&labelColor=000000&logo=data%3Aimage%2Fsvg%2Bxml%3Bbase64%2CPHN2ZyB3aWR0aD0iMTYiIGhlaWdodD0iMTYiIHZpZXdCb3g9IjAgMCAxNiAxNiIgZmlsbD0ibm9uZSIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj4KPHBhdGggZD0iTTQuOTYxNTYgMS42MDAxSDIuMjQxNTZDMS44ODgxIDEuNjAwMSAxLjYwMTU2IDEuODg2NjQgMS42MDE1NiAyLjI0MDFWNC45NjAxQzEuNjAxNTYgNS4zMTM1NiAxLjg4ODEgNS42MDAxIDIuMjQxNTYgNS42MDAxSDQuOTYxNTZDNS4zMTUwMiA1LjYwMDEgNS42MDE1NiA1LjMxMzU2IDUuNjAxNTYgNC45NjAxVjIuMjQwMUM1LjYwMTU2IDEuODg2NjQgNS4zMTUwMiAxLjYwMDEgNC45NjE1NiAxLjYwMDFaIiBmaWxsPSIjZmZmIi8%2BCjxwYXRoIGQ9Ik00Ljk2MTU2IDEwLjM5OTlIMi4yNDE1NkMxLjg4ODEgMTAuMzk5OSAxLjYwMTU2IDEwLjY4NjQgMS42MDE1NiAxMS4wMzk5VjEzLjc1OTlDMS42MDE1NiAxNC4xMTM0IDEuODg4MSAxNC4zOTk5IDIuMjQxNTYgMTQuMzk5OUg0Ljk2MTU2QzUuMzE1MDIgMTQuMzk5OSA1LjYwMTU2IDE0LjExMzQgNS42MDE1NiAxMy43NTk5VjExLjAzOTlDNS42MDE1NiAxMC42ODY0IDUuMzE1MDIgMTAuMzk5OSA0Ljk2MTU2IDEwLjM5OTlaIiBmaWxsPSIjZmZmIi8%2BCjxwYXRoIGQ9Ik0xMy43NTg0IDEuNjAwMUgxMS4wMzg0QzEwLjY4NSAxLjYwMDEgMTAuMzk4NCAxLjg4NjY0IDEwLjM5ODQgMi4yNDAxVjQuOTYwMUMxMC4zOTg0IDUuMzEzNTYgMTAuNjg1IDUuNjAwMSAxMS4wMzg0IDUuNjAwMUgxMy43NTg0QzE0LjExMTkgNS42MDAxIDE0LjM5ODQgNS4zMTM1NiAxNC4zOTg0IDQuOTYwMVYyLjI0MDFDMTQuMzk4NCAxLjg4NjY0IDE0LjExMTkgMS42MDAxIDEzLjc1ODQgMS42MDAxWiIgZmlsbD0iI2ZmZiIvPgo8cGF0aCBkPSJNNCAxMkwxMiA0TDQgMTJaIiBmaWxsPSIjZmZmIi8%2BCjxwYXRoIGQ9Ik00IDEyTDEyIDQiIHN0cm9rZT0iI2ZmZiIgc3Ryb2tlLXdpZHRoPSIxLjUiIHN0cm9rZS1saW5lY2FwPSJyb3VuZCIvPgo8L3N2Zz4K&logoColor=ffffff)](https://zread.ai/looplj/axonhub)


---

## 🎯 演示 | Demo

在我们的 [演示实例](https://axonhub.onrender.com) 上体验 AxonHub！

**注意**：演示网站目前配置了 Zhipu 和 OpenRouter 的免费模型。

### 演示账号 | Demo Account
- **邮箱 Email**: demo@example.com
- **密码 Password**: 12345678

---

## ⭐ 特性 | Features

### 📸 截图 | Screenshots

以下是 AxonHub 的实际运行截图：

<table>
  <tr>
    <td align="center">
      <a href="docs/screenshots/axonhub-dashboard.png">
        <img src="docs/screenshots/axonhub-dashboard.png" alt="系统仪表板" width="250"/>
      </a>
      <br/>
      系统仪表板
    </td>
    <td align="center">
      <a href="docs/screenshots/axonhub-channels.png">
        <img src="docs/screenshots/axonhub-channels.png" alt="渠道管理" width="250"/>
      </a>
      <br/>
      渠道管理
    </td>
    <td align="center">
      <a href="docs/screenshots/axonhub-model-price.png">
        <img src="docs/screenshots/axonhub-model-price.png" alt="模型价格" width="250"/>
      </a>
      <br/>
      模型价格
    </td>
  </tr>
  <tr>
   <td align="center">
      <a href="docs/screenshots/axonhub-models.png">
        <img src="docs/screenshots/axonhub-models.png" alt="模型" width="250"/>
      </a>
      <br/>
      模型
    </td>
    <td align="center">
      <a href="docs/screenshots/axonhub-trace.png">
        <img src="docs/screenshots/axonhub-trace.png" alt="追踪查看" width="250"/>
      </a>
      <br/>
      追踪查看
    </td>
    <td align="center">
      <a href="docs/screenshots/axonhub-requests.png">
        <img src="docs/screenshots/axonhub-requests.png" alt="请求监控" width="250"/>
      </a>
      <br/>
      请求监控
    </td>
    
  </tr>
</table>

---

### 🚀 API 类型 | API Types

| API 类型 | 状态 | 描述 | 文档 |
|---------|--------|-------------|--------|
| **文本生成（Text Generation）** | ✅ Done | 对话交互接口 | [OpenAI API](docs/zh/api-reference/openai-api.md)、[Anthropic API](docs/zh/api-reference/anthropic-api.md)、[Gemini API](docs/zh/api-reference/gemini-api.md) |
| **图片生成（Image Generation）** | 📝 Todo | 图片生成 | [Image Generation](docs/zh/api-reference/image-generation.md) |
| **重排序（Rerank）** | ✅ Done | 结果排序 | [Rerank API](docs/zh/api-reference/rerank-api.md) |
| **嵌入（Embedding）** | ✅ Done | 向量嵌入生成 | [Embedding API](docs/zh/api-reference/embedding-api.md) |
| **实时对话（Realtime）** | 📝 Todo | 实时对话功能 | - |

---

### 🤖 支持的提供商 | Supported Providers

| 提供商 Provider        | 状态 Status | 支持模型 Models              | 兼容 API |
| ---------------------- | ---------- | ---------------------------- | --------------- |
| **OpenAI**             | ✅ 已完成   | GPT-4, GPT-4o, GPT-5 等      | OpenAI, Anthropic, Gemini, Embedding, Image Generation |
| **Anthropic**          | ✅ 已完成   | Claude 3.5, Claude 3.0 等    | OpenAI, Anthropic, Gemini |
| **智谱 AI (Zhipu)**    | ✅ 已完成   | GLM-4.5, GLM-4.5-air 等      | OpenAI, Anthropic, Gemini |
| **月之暗面 (Moonshot)** | ✅ 已完成   | kimi-k2 等                   | OpenAI, Anthropic, Gemini |
| **DeepSeek**           | ✅ 已完成   | DeepSeek-V3.1 等             | OpenAI, Anthropic, Gemini |
| **字节跳动豆包**        | ✅ 已完成   | doubao-1.6 等                | OpenAI, Anthropic, Gemini, Image Generation |
| **Gemini**             | ✅ 已完成   | Gemini 2.5 等                | OpenAI, Anthropic, Gemini, Image Generation |
| **Jina AI**            | ✅ 已完成   | Embeddings, Reranker 等      | Jina Embedding, Jina Rerank |
| **OpenRouter**         | ✅ 已完成   | 多种模型                     | OpenAI, Anthropic, Gemini, Image Generation |
| **ZAI**                | ✅ 已完成   | -                            | Image Generation |
| **AWS Bedrock**        | 🔄 测试中  | Claude on AWS                | OpenAI, Anthropic, Gemini |
| **Google Cloud**       | 🔄 测试中  | Claude on GCP                | OpenAI, Anthropic, Gemini |
| **NanoGPT**            | ✅ 已完成  | 多种模型、图像生成             | OpenAI, Anthropic, Gemini, Image Generation |

---


## 🚀 快速开始 | Quick Start

### 30 秒本地启动 | 30-Second Local Start

```bash
# 下载并解压（以 macOS ARM64 为例）
curl -sSL https://github.com/looplj/axonhub/releases/latest/download/axonhub_darwin_arm64.tar.gz | tar xz
cd axonhub_*

# 使用 SQLite 运行（默认）
./axonhub

# 打开 http://localhost:8090
# 首次运行：按照初始化向导设置系统（创建管理员账号，密码至少需要 6 位）
```

就这样！现在配置你的第一个 AI 渠道，开始通过 AxonHub 调用模型。

### Rust 切换交付物

打标签发布时，会提供面向当前受支持替代范围的 Rust 交付路径：

- 形如 `axonhub-rust_<tag>_<platform>.(tar.gz|zip)` 的发布资产
- Docker 镜像 `ghcr.io/looplj/axonhub:rust-latest` 与 `ghcr.io/looplj/axonhub:rust-<tag>`
- `docker-compose.rust.yml` 中的 Compose 示例

这些交付物会保留 Rust CLI / 配置契约，并交付当前 Rust 测试矩阵已验证的 SQLite 与 PostgreSQL 替代能力面：`/health`、admin bootstrap/status/auth/read 流程、admin GraphQL、OpenAPI GraphQL、request-context/auth 基础能力，以及已迁移的 inference 路由族（包含 `POST /v1/images/generations`）。同一 SeaORM 支持切片也已经为 MySQL 布线，但 Rust 测试套件中完整的 MySQL 自动化集成验证仍待完成。任何超出该受支持范围的路由族，仍会由 Rust 返回显式 `501 Not Implemented`，直到后续单独迁移并验证完成。

这些 Rust 交付物对应的二元 PASS/FAIL 切换、HOLD 与 ROLLBACK 条件，统一定义在 `.sisyphus/artifacts/rust-backend-seaorm-actix-migration-plan/final-cutover-gates.md`。

### 零代码迁移示例 | Zero-Code Migration Example

**你的现有代码无需任何改动。** 只需将 SDK 指向 AxonHub：

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:8090/v1",  # 指向 AxonHub
    api_key="your-axonhub-api-key"        # 使用 AxonHub API 密钥
)

# 用 OpenAI SDK 调用 Claude！
response = client.chat.completions.create(
    model="claude-3-5-sonnet",  # 或 gpt-4、gemini-pro、deepseek-chat...
    messages=[{"role": "user", "content": "Hello!"}]
)
```

切换模型只需改一行：`model="gpt-4"` → `model="claude-3-5-sonnet"`。无需改动 SDK。

---

## 🚀 部署指南 | Deployment Guide

### 💻 个人电脑部署 | Personal Computer Deployment

适合个人开发者和小团队使用，无需复杂配置。

#### 快速下载运行 | Quick Download & Run

1. **下载最新版本** 从 [GitHub Releases](https://github.com/looplj/axonhub/releases)
   - 选择适合您操作系统的版本：

2. **解压并运行**
   ```bash
   # 解压下载的文件
   unzip axonhub_*.zip
   cd axonhub_*
   
   # 添加执行权限 (仅限 Linux/macOS)
   chmod +x axonhub
   
   # 直接运行 - 默认使用 SQLite 数据库
   # 安装 AxonHub 到系统
   ./install.sh

   # 启动 AxonHub 服务
   ./start.sh

   # 停止 AxonHub 服务
   ./stop.sh
   ```

3. **访问应用**
   ```
   http://localhost:8090
   ```

---

### 🖥️ 服务器部署 | Server Deployment

适用于生产环境、高可用性和企业级部署。

#### 数据库支持 | Database Support

> **重要：** 当前 Rust 切换已验证 **SQLite 和 PostgreSQL** 在接受的切片中。MySQL 通过相同的 SeaORM 支持切片已布线，但完整的 Rust 端自动化集成验证仍在进行中。TiDB 和 Neon DB 保持为旧版 Go 专属，直到在 Rust 中单独验证。详见 [后端迁移状态](#后端迁移状态--backend-migration-status) 部分。

**Rust 切换（当前受支持范围）：**

| 数据库 | 支持版本 | 推荐场景 | 自动迁移 | 链接 |
| -------- | ------------------ | -------------------- | -------------- | ------ |
| **SQLite** | 3.0+ | 开发环境、小型部署、Rust 切换范围 | ✅ 支持 | [SQLite](https://www.sqlite.org/index.html) |
| **PostgreSQL** | 15+ | 生产环境、中大型部署、Rust 切换范围 | ✅ 支持 | [PostgreSQL](https://www.postgresql.org/) |

**Rust 切换（通过共享 SeaORM 接缝已实现，尚未完全集成验证）：**

| 数据库 | 支持版本 | 推荐场景 | 自动迁移 | 链接 |
| -------- | ------------------ | -------------------- | -------------- | ------ |
| **MySQL** | 8.0+ | 与 SQLite/PostgreSQL 相同的 SeaORM 支持切片；在生产使用前请在您的环境中验证 | ⚠️ 已实现，仓库级集成验证待完成 | [MySQL](https://www.mysql.com/) |

**旧版 Go 后端（Rust 切换尚未支持）：**

| 数据库 | 支持版本 | 推荐场景 | 自动迁移 | 链接 |
| -------- | ------------------ | -------------------- | -------------- | ------ |
| **TiDB Cloud** | Starter | Serverless, Free tier, Auto Scale | ✅ 支持 | [TiDB Cloud](https://www.pingcap.com/tidb-cloud-starter/) |
| **TiDB Cloud** | Dedicated | 分布式部署、大规模 | ✅ 支持 | [TiDB Cloud](https://www.pingcap.com/tidb-cloud-dedicated/) |
| **TiDB** | V8.0+ | 分布式部署、大规模 | ✅ 支持 | [TiDB](https://tidb.io/) |
| **Neon DB** | - | Serverless, Free tier, Auto Scale | ✅ 支持 | [Neon DB](https://neon.com/) |

#### 配置文件 | Configuration

**Rust 切换（SQLite）：**

AxonHub 使用 YAML 配置文件，支持环境变量覆盖。Rust 切换默认使用 SQLite，无需额外配置。

```yaml
# config.yml
server:
  port: 8090
  name: "AxonHub"
  debug: false

# SQLite 是 Rust 切换的默认数据库。
# PostgreSQL 也经过验证，使用相同的 db.dialect/db.dsn 契约。
# MySQL 已通过同一契约布线，但尚未完全集成验证。
# 无需额外 db 配置 - 数据存储在 ./axonhub.db（SQLite 默认）
```

环境变量（可选）：

```bash
AXONHUB_SERVER_PORT=8090
AXONHUB_LOG_LEVEL=info
# SQLite 无需数据库配置 - 它是默认值
```

**旧版 Go 后端（多 Dialect）：**

对于 TiDB 和 Neon DB 部署，必须使用旧版 Go 后端。配置示例请参阅 [配置文档](docs/zh/deployment/configuration.md)。

#### Docker Compose 部署

**Rust 切换（SQLite）：**

使用提供的 `docker-compose.rust.yml` 文件部署 Rust 后端（SQLite）：

```bash
# 启动 Rust 后端（SQLite）
docker-compose -f docker-compose.rust.yml up -d

# 查看状态
docker-compose -f docker-compose.rust.yml ps
```

**旧版 Go 后端（多 Dialect）：**

对于 TiDB 和 Neon DB 部署，请使用旧版 Go 后端。配置示例请参阅 [部署文档](docs/zh/deployment/configuration.md)。

#### Helm Kubernetes 部署 | Helm Kubernetes Deployment

**仅限旧版 Go 后端（Rust 切换尚未支持）：**

使用官方 Helm Chart 在 Kubernetes 上部署 AxonHub。此部署路径使用旧版 Go 后端并支持多 dialect 数据库。

```bash
# 快速安装
git clone https://github.com/looplj/axonhub.git
cd axonhub
helm install axonhub ./deploy/helm

# 生产部署
helm install axonhub ./deploy/helm -f ./deploy/helm/values-production.yaml

# 访问 AxonHub
kubectl port-forward svc/axonhub 8090:8090
# 访问 http://localhost:8090
```

**关键配置选项：**

| 参数 | 描述 | 默认 |
|-----------|-------------|---------|
| `axonhub.replicaCount` | 副本数 | `1` |
| `axonhub.dbPassword` | 数据库密码 | `axonhub_password` |
| `postgresql.enabled` | 是否启用内嵌 PostgreSQL（仅 Go 后端） | `true` |
| `ingress.enabled` | 是否启用 Ingress | `false` |
| `persistence.enabled` | 是否启用持久化存储 | `false` |

有关详细配置和故障排查，请参阅 [Helm Chart 文档](deploy/helm/README.md)。注意：Helm 部署暂不支持 Rust 切换。

#### 虚拟机部署 | Virtual Machine Deployment

**Rust 切换（SQLite）：**

从 [GitHub Releases](https://github.com/looplj/axonhub/releases) 下载 Rust 专用版本（查找 `axonhub-rust_*` 资产）。

```bash
# 提取并运行
unzip axonhub-rust_*.zip
cd axonhub-rust_*

# 安装
sudo ./install.sh

# 配置文件检查（可选）
axonhub config validate

# 使用管理脚本管理 AxonHub

# 启动
./start.sh

# 停止
./stop.sh
```

**旧版 Go 后端（多 Dialect）：**

对于 TiDB 和 Neon DB 部署，请使用标准 `axonhub_*` 版本并按 [配置文档](docs/zh/deployment/configuration.md) 配置数据库连接。

---

## 📖 使用指南 | Usage Guide

### 1. 初始化设置 | Initial Setup

1. **访问管理界面**
   ```
   http://localhost:8090
   ```

2. **配置 AI 提供商**
   - 在管理界面中添加 API 密钥
   - 测试连接确保配置正确

3. **创建用户和角色**
   - 设置权限管理
   - 分配适当的访问权限

### 2. Channel 配置 | Channel Configuration

在管理界面中配置 AI 提供商渠道。关于渠道配置的详细信息，包括模型映射、参数覆盖和故障排除，请参阅 [渠道配置指南](docs/zh/guides/channel-management.md)。

### 3. 模型管理 | Model Management

AxonHub 提供灵活的模型管理系统，支持通过模型关联将抽象模型映射到特定渠道和模型实现。这使您能够：

- **统一模型接口** - 使用抽象模型 ID（如 `gpt-4`、`claude-3-opus`）替代渠道特定的名称
- **智能渠道选择** - 基于关联规则和负载均衡自动将请求路由到最优渠道
- **灵活的映射策略** - 支持精确的渠道-模型匹配、正则表达式模式和基于标签的选择
- **基于优先级的回退** - 配置多个具有优先级的关联以实现自动故障转移

关于模型管理的全面信息，包括关联类型、配置示例和最佳实践，请参阅 [模型管理指南](docs/zh/guides/model-management.md)。

### 4. 创建 API Key | Create API Keys

创建 API 密钥以验证您的应用程序与 AxonHub 的连接。每个 API 密钥可以配置多个配置文件（Profile），用于定义：

- **模型映射** - 使用精确匹配或正则表达式模式将用户请求的模型转换为实际可用的模型
- **渠道限制** - 通过渠道 ID 或标签限制 API 密钥可以使用的渠道
- **模型访问控制** - 控制特定配置文件可以访问的模型
- **配置文件切换** - 通过激活不同的配置文件即时更改行为

关于 API 密钥配置文件的详细信息，包括配置示例、验证规则和最佳实践，请参阅 [API 密钥配置文件指南](docs/zh/guides/api-key-profiles.md)。

### 5. AI 编程工具集成 | AI Coding Tools Integration

关于如何在 OpenCode、Claude Code 与 Claude Codex 中配置与 AxonHub 的集成、排查常见问题以及结合模型配置文件工作流的最佳实践，请参阅专门的集成指南：
- [OpenCode 集成指南](docs/zh/guides/opencode-integration.md)
- [Claude Code 集成指南](docs/zh/guides/claude-code-integration.md)
- [Codex 集成指南](docs/zh/guides/codex-integration.md)

这些文档提供了环境变量示例、Codex 配置模板、模型配置文件说明以及工作流示例，帮助您快速完成接入。

---

### 6. 使用 SDK | SDK Usage

详细的 SDK 使用示例和代码示例，请参阅 API 文档：
- [OpenAI API](docs/zh/api-reference/openai-api.md)
- [Anthropic API](docs/zh/api-reference/anthropic-api.md)
- [Gemini API](docs/zh/api-reference/gemini-api.md)


## 🛠️ 开发指南

详细的开发说明、架构设计和贡献指南，请查看 [docs/zh/development/development.md](docs/zh/development/development.md)。

---

## 🤝 致谢 | Acknowledgments

- 🙏 [musistudio/llms](https://github.com/musistudio/llms) - LLM 转换框架，灵感来源
- 🎨 [satnaing/shadcn-admin](https://github.com/satnaing/shadcn-admin) - 管理界面模板
- 🔧 [99designs/gqlgen](https://github.com/99designs/gqlgen) - 旧后端使用的 GraphQL 代码生成
- 🌐 [gin-gonic/gin](https://github.com/gin-gonic/gin) - 旧后端使用的 HTTP 框架
- 🗄️ [ent/ent](https://github.com/ent/ent) - 旧后端使用的 ORM 框架
- 🦀 [tokio-rs/axum](https://github.com/tokio-rs/axum) - Rust 迁移切片早期阶段使用的 HTTP 框架
- ⚙️ [actix-rs/actix-web](https://github.com/actix/actix-web) - Rust 后端切换生产环境使用的 HTTP 框架
- ⚙️ [tokio-rs/tokio](https://github.com/tokio-rs/tokio) - Rust 迁移切片使用的异步运行时
- ☁️ [render](https://render.com) - 免费云部署平台，用于部署 demo
- 🗄️ [tidbcloud](https://www.pingcap.com/tidb-cloud/) - Serverless 数据库平台，用于部署 demo

---

## 📄 许可证 | License

本项目采用多种许可证授权（Apache-2.0 和 LGPL-3.0）。详见 [LICENSE](LICENSE) 文件了解详细的项目授权说明与条款。
---

<div align="center">

**AxonHub** - All-in-one AI 开发平台，让 AI 开发更简单

[🏠 官网](https://github.com/looplj/axonhub) • [📚 文档](https://deepwiki.com/looplj/axonhub) • [🐛 问题反馈](https://github.com/looplj/axonhub/issues)

Built with ❤️ by the AxonHub team

</div>
