# 快速入门指南

## 开始之前

AxonHub 当前正处于增量式 Go → Rust 后端迁移阶段。

- 如果你要体验**完整产品能力**，请使用 Docker 或发布版二进制。
- 如果你要参与**Rust 迁移切片开发**，请使用仓库中的 Cargo workspace。

Rust 切片目前已经提供配置加载、CLI 形状兼容、`/health`、`GET /admin/system/status` 以及对未迁移路由族的显式 `501` 返回，但它**还没有**完整 API 对等能力。

## 前置要求

- Docker 和 Docker Compose，用于完整本地产品体验
- 或 Rust 1.78+、Go 1.26+、Node.js 18+、pnpm，用于仓库开发
- 来自 AI 提供商的有效 API 密钥

## 最快路径：完整本地运行

### 1. 克隆仓库

```bash
git clone https://github.com/looplj/axonhub.git
cd axonhub
```

### 2. 准备配置

```bash
cp config.example.yml config.yml
```

### 3. 启动服务栈

```bash
docker-compose up -d
```

### 4. 打开 AxonHub

- Web 界面：`http://localhost:8090`

## Rust 迁移切片快速开始

如果你在开发新的 Rust 后端切片：

```bash
cargo run -p axonhub-server -- help
cargo run -p axonhub-server -- config preview
cargo run -p axonhub-server -- config validate
cargo run -p axonhub-server --
```

当前 Rust 切片的行为预期：

- `/health` 可用
- 对受支持的 SQLite 迁移路径，`GET /admin/system/status` 可用
- 支持配置文件搜索路径与 `AXONHUB_*` 环境变量
- 未迁移路由族返回结构化 `501 Not Implemented` JSON

## 产品的第一步

当完整后端运行起来后，AxonHub 的正常接入流程仍然不变：

1. 配置第一个 provider channel，
2. 创建 API Key，
3. 将 SDK 指向 AxonHub，
4. 通过统一 API 转发请求。

## API 调用示例

```python
from openai import OpenAI

client = OpenAI(
    api_key="your-axonhub-api-key",
    base_url="http://localhost:8090/v1"
)

response = client.chat.completions.create(
    model="gpt-4o",
    messages=[
        {"role": "user", "content": "Hello, AxonHub!"}
    ]
)

print(response.choices[0].message.content)
```

## 这次迁移改变了什么

这次迁移改变的是后端实现方式，而不是 AxonHub 的产品目标。

- 产品文档仍然描述完整的 AxonHub 能力；
- Rust workspace 是新的实现路径；
- 在更多路由族完成迁移前，Go 后端仍然是完整运行时。

## 相关文档

- [配置指南](../deployment/configuration.md)
- [Docker 部署](../deployment/docker.md)
- [开发指南](../development/development.md)
