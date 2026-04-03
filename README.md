<div align="center">

# AxonHub - All-in-one AI Development Platform
### Use any SDK. Access any model. Zero code changes.

<a href="https://trendshift.io/repositories/16225" target="_blank"><img src="https://trendshift.io/api/badge/repositories/16225" alt="looplj%2Faxonhub | Trendshift" style="width: 250px; height: 55px;" width="250" height="55"/></a>

</div>

<div align="center">

[![Test Status](https://github.com/looplj/axonhub/actions/workflows/test.yml/badge.svg)](https://github.com/looplj/axonhub/actions/workflows/test.yml)
[![Lint Status](https://github.com/looplj/axonhub/actions/workflows/lint.yml/badge.svg)](https://github.com/looplj/axonhub/actions/workflows/lint.yml)
[![Rust Workspace](https://img.shields.io/badge/rust-workspace-d19132?logo=rust&logoColor=white)](Cargo.toml)
[![Go Backend (Legacy)](https://img.shields.io/badge/go-backend-legacy-00ADD8?logo=go&logoColor=white)](cmd/axonhub/main.go)
[![Docker Ready](https://img.shields.io/badge/docker-ready-2496ED?logo=docker&logoColor=white)](https://docker.com)

[English](README.md) | [中文](README.zh-CN.md)

</div>

---

## Backend Architecture

AxonHub's canonical backend implementation is written in Rust. The Rust workspace and Rust-tagged release artifacts constitute the primary deployment path for the platform.

### Current Rust-Supported Surface (Verified)

The Rust backend provides complete, verified functionality for SQLite and PostgreSQL deployments:

- **CLI/config**: command-line interface and configuration loading
- **Health & system**: `/health`, `GET /admin/system/status`, `POST /admin/system/initialize`
- **Identity & context**: authentication, request context, JWT handling
- **Admin read routes**: `GET /admin/requests/:request_id/content`
- **Admin GraphQL**: `POST /admin/graphql` with playground and full support for settings management, user management, project/role management, quota configuration, operational mutations (backup, restore, GC cleanup, etc.)
- **OpenAPI GraphQL**: `POST /openapi/v1/graphql` with playground
- **OpenAI-compatible `/v1` inference**: `/models`, `/chat/completions`, `/responses`, `/responses/compact`, `/embeddings`, `/messages`, `/rerank`, `/images/generations`, `/images/edits`, `/images/variations`, `/v1/realtime` (JSON POST), realtime WebSocket upgrade and session management (`/v1/realtime/sessions`), video generation
- **Other provider APIs**: Jina, Anthropic, Gemini, Doubao routes as listed in routes
- **AiSDK compatibility**: Full support for Vercel AI SDK protocol via `X-Vercel-Ai-Ui-Message-Stream` and `X-Vercel-AI-Data-Stream` headers
- **Provider-edge admin OAuth**: Codex, Claude Code, Antigravity, Copilot
- **RBAC & permissions**: fine-grained access control with system and project scopes
- **Business logic**: channel/model association & fetching, usage & cost tracking, trace/thread management, system onboarding
- **Transformer/pipeline**: provider orchestration, outbound transformers, middleware pipeline
- **Enterprise features**: prompt protection, provider quota management, circuit breakers
- **Configuration**: full alignment with AxonHub configuration surface for SQLite/PostgreSQL
- **Database support**: SQLite and PostgreSQL (canonical); MySQL/TiDB/Neon are not supported in the Rust backend

### Historical Reference Only

The legacy Go tree under `cmd/axonhub/main.go`, `conf/conf.go`, and `internal/server/**` remains in-repo as contract/oracle history and implementation reference. It is not a supported deployment path, release path, or canonical backend for AxonHub.

### What This Means in Practice

For the **supported product surface** described above, use the Rust binary or the Rust-tagged release assets.

The Rust backend provides complete support for all core features. The legacy Go tree remains available in-repo as historical reference only and is not a fallback runtime.

---

> NOTE
>
> 1. This project is maintained by an individual. The author makes no warranties and assumes no liability for risks arising from its use. Please evaluate carefully.
> 2. The core scope of this project does not include 2api (subscription-to-API conversion). If you need that, consider other open-source projects focused on 2api.

---

## 📖 Project Introduction

### All-in-one AI Development Platform

**AxonHub is the AI gateway that lets you switch between model providers without changing a single line of code.**

Whether you're using OpenAI SDK, Anthropic SDK, or any AI SDK, AxonHub transparently translates your requests to work with any supported model provider. No refactoring, no SDK swaps—just change a configuration and you're done.

**What it solves:**
- 🔒 **Vendor lock-in** - Switch from GPT-4 to Claude or Gemini instantly
- 🔧 **Integration complexity** - One API format for 10+ providers
- 📊 **Observability gap** - Complete request tracing out of the box
- 💸 **Cost control** - Real-time usage tracking and budget management

<div align="center">
  <img src="docs/axonhub-architecture-light.svg" alt="AxonHub Architecture" width="700"/>
</div>

### Core Features

| Feature | What You Get |
|---------|-------------|
| 🔄 [**Any SDK → Any Model**](docs/en/api-reference/openai-api.md) | Use OpenAI SDK to call Claude, or Anthropic SDK to call GPT. Zero code changes. |
| 🔍 [**Full Request Tracing**](docs/en/guides/tracing.md) | Complete request timelines with thread-aware observability. Debug faster. |
| 🔐 [**Enterprise RBAC**](docs/en/guides/permissions.md) | Fine-grained access control, usage quotas, and data isolation. |
| ⚡ [**Smart Load Balancing**](docs/en/guides/load-balance.md) | Auto failover in <100ms. Always route to the healthiest channel. |
| 💰 [**Real-time Cost Tracking**](docs/en/guides/cost-tracking.md) | Per-request cost breakdown. Input, output, cache tokens—all tracked. |

---

## 📚 Documentation

For detailed technical documentation, API references, architecture design, and more, please visit
- [![DeepWiki](https://img.shields.io/badge/DeepWiki-looplj%2Faxonhub-blue.svg?logo=data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACwAAAAyCAYAAAAnWDnqAAAAAXNSR0IArs4c6QAAA05JREFUaEPtmUtyEzEQhtWTQyQLHNak2AB7ZnyXZMEjXMGeK/AIi+QuHrMnbChYY7MIh8g01fJoopFb0uhhEqqcbWTp06/uv1saEDv4O3n3dV60RfP947Mm9/SQc0ICFQgzfc4CYZoTPAswgSJCCUJUnAAoRHOAUOcATwbmVLWdGoH//PB8mnKqScAhsD0kYP3j/Yt5LPQe2KvcXmGvRHcDnpxfL2zOYJ1mFwrryWTz0advv1Ut4CJgf5uhDuDj5eUcAUoahrdY/56ebRWeraTjMt/00Sh3UDtjgHtQNHwcRGOC98BJEAEymycmYcWwOprTgcB6VZ5JK5TAJ+fXGLBm3FDAmn6oPPjR4rKCAoJCal2eAiQp2x0vxTPB3ALO2CRkwmDy5WohzBDwSEFKRwPbknEggCPB/imwrycgxX2NzoMCHhPkDwqYMr9tRcP5qNrMZHkVnOjRMWwLCcr8ohBVb1OMjxLwGCvjTikrsBOiA6fNyCrm8V1rP93iVPpwaE+gO0SsWmPiXB+jikdf6SizrT5qKasx5j8ABbHpFTx+vFXp9EnYQmLx02h1QTTrl6eDqxLnGjporxl3NL3agEvXdT0WmEost648sQOYAeJS9Q7bfUVoMGnjo4AZdUMQku50McDcMWcBPvr0SzbTAFDfvJqwLzgxwATnCgnp4wDl6Aa+Ax283gghmj+vj7feE2KBBRMW3FzOpLOADl0Isb5587h/U4gGvkt5v60Z1VLG8BhYjbzRwyQZemwAd6cCR5/XFWLYZRIMpX39AR0tjaGGiGzLVyhse5C9RKC6ai42ppWPKiBagOvaYk8lO7DajerabOZP46Lby5wKjw1HCRx7p9sVMOWGzb/vA1hwiWc6jm3MvQDTogQkiqIhJV0nBQBTU+3okKCFDy9WwferkHjtxib7t3xIUQtHxnIwtx4mpg26/HfwVNVDb4oI9RHmx5WGelRVlrtiw43zboCLaxv46AZeB3IlTkwouebTr1y2NjSpHz68WNFjHvupy3q8TFn3Hos2IAk4Ju5dCo8B3wP7VPr/FGaKiG+T+v+TQqIrOqMTL1VdWV1DdmcbO8KXBz6esmYWYKPwDL5b5FA1a0hwapHiom0r/cKaoqr+27/XcrS5UwSMbQAAAABJRU5ErkJggg==)](https://deepwiki.com/looplj/axonhub)
- [![zread](https://img.shields.io/badge/Ask_Zread-_.svg?style=flat&color=00b0aa&labelColor=000000&logo=data%3Aimage%2Fsvg%2Bxml%3Bbase64%2CPHN2ZyB3aWR0aD0iMTYiIGhlaWdodD0iMTYiIHZpZXdCb3g9IjAgMCAxNiAxNiIgZmlsbD0ibm9uZSIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj4KPHBhdGggZD0iTTQuOTYxNTYgMS42MDAxSDIuMjQxNTZDMS44ODgxIDEuNjAwMSAxLjYwMTU2IDEuODg2NjQgMS42MDE1NiAyLjI0MDFWNC45NjAxQzEuNjAxNTYgNS4zMTM1NiAxLjg4ODEgNS42MDAxIDIuMjQxNTYgNS42MDAxSDQuOTYxNTZDNS4zMTUwMiA1LjYwMDEgNS42MDE1NiA1LjMxMzU2IDUuNjAxNTYgNC45NjAxVjIuMjQwMUM1LjYwMTU2IDEuODg2NjQgNS4zMTUwMiAxLjYwMDEgNC45NjE1NiAxLjYwMDFaIiBmaWxsPSIjZmZmIi8%2BCjxwYXRoIGQ9Ik00Ljk2MTU2IDEwLjM5OTlIMi4yNDE1NkMxLjg4ODEgMTAuMzk5OSAxLjYwMTU2IDEwLjY4NjQgMS42MDE1NiAxMS4wMzk5VjEzLjc1OTlDMS42MDE1NiAxNC4xMTM0IDEuODg4MSAxNC4zOTk5IDIuMjQxNTYgMTQuMzk5OUg0Ljk2MTU2QzUuMzE1MDIgMTQuMzk5OSA1LjYwMTU2IDE0LjExMzQgNS42MDE1NiAxMy43NTk5VjExLjAzOTlDNS42MDE1NiAxMC42ODY0IDUuMzE1MDIgMTAuMzk5OSA0Ljk2MTU2IDEwLjM5OTlaIiBmaWxsPSIjZmZmIi8%2BCjxwYXRoIGQ9Ik0xMy43NTg0IDEuNjAwMUgxMS4wMzg0QzEwLjY4NSAxLjYwMDEgMTAuMzk4NCAxLjg4NjY0IDEwLjM5ODQgMi4yNDAxVjQuOTYwMUMxMC4zOTg0IDUuMzEzNTYgMTAuNjg1IDUuNjAwMSAxMS4wMzg0IDUuNjAwMUgxMy43NTg0QzE0LjExMTkgNS42MDAxIDE0LjM5ODQgNS4zMTM1NiAxNC4zOTg0IDQuOTYwMVYyLjI0MDFDMTQuMzk4NCAxLjg4NjY0IDE0LjExMTkgMS42MDAxIDEzLjc1ODQgMS42MDAxWiIgZmlsbD0iI2ZmZiIvPgo8cGF0aCBkPSJNNCAxMkwxMiA0TDQgMTJaIiBmaWxsPSIjZmZmIi8%2BCjxwYXRoIGQ9Ik00IDEyTDEyIDQiIHN0cm9rZT0iI2ZmZiIgc3Ryb2tlLXdpZHRoPSIxLjUiIHN0cm9rZS1saW5lY2FwPSJyb3VuZCIvPgo8L3N2Zz4K&logoColor=ffffff)](https://zread.ai/looplj/axonhub)

---

## 🎯 Demo

Try AxonHub live at our [demo instance](https://axonhub.onrender.com)!

**Note**：The demo instance currently configures Zhipu and OpenRouter free models.

### Demo Account

- **Email**: demo@example.com
- **Password**: 12345678

---

## ⭐ Features

### 📸 Screenshots

Here are some screenshots of AxonHub in action:

<table>
  <tr>
    <td align="center">
      <a href="docs/screenshots/axonhub-dashboard.png">
        <img src="docs/screenshots/axonhub-dashboard.png" alt="System Dashboard" width="250"/>
      </a>
      <br/>
      System Dashboard
    </td>
    <td align="center">
      <a href="docs/screenshots/axonhub-channels.png">
        <img src="docs/screenshots/axonhub-channels.png" alt="Channel Management" width="250"/>
      </a>
      <br/>
      Channel Management
    </td>
    <td align="center">
      <a href="docs/screenshots/axonhub-model-price.png">
        <img src="docs/screenshots/axonhub-model-price.png" alt="Model Price" width="250"/>
      </a>
      <br/>
      Model Price
    </td>
  </tr>
  <tr>
  <td align="center">
      <a href="docs/screenshots/axonhub-models.png">
        <img src="docs/screenshots/axonhub-models.png" alt="Models" width="250"/>
      </a>
      <br/>
      Models
    </td>
    <td align="center">
      <a href="docs/screenshots/axonhub-trace.png">
        <img src="docs/screenshots/axonhub-trace.png" alt="Trace Viewer" width="250"/>
      </a>
      <br/>
      Trace Viewer
    </td>
    <td align="center">
      <a href="docs/screenshots/axonhub-requests.png">
        <img src="docs/screenshots/axonhub-requests.png" alt="Request Monitoring" width="250"/>
      </a>
      <br/>
      Request Monitoring
    </td>
  </tr>
</table>

---

### 🚀 API Types

| API Type             | Status     | Description                    | Document                                     |
| -------------------- | ---------- | ------------------------------ | -------------------------------------------- |
| **Text Generation**  | ✅ Done    | Conversational interface       | [OpenAI API](docs/en/api-reference/openai-api.md), [Anthropic API](docs/en/api-reference/anthropic-api.md), [Gemini API](docs/en/api-reference/gemini-api.md) |
| **Image Generation** | ✅ Done    | Image generation, editing, variations | [Image Generation](docs/en/api-reference/image-generation.md) |
| **Rerank**           | ✅ Done    | Results ranking                | [Rerank API](docs/en/api-reference/rerank-api.md) |
| **Embedding**        | ✅ Done    | Vector embedding generation    | [Embedding API](docs/en/api-reference/embedding-api.md) |
| **Realtime**         | ✅ Done    | Live conversation capabilities (WebSocket, sessions) | [OpenAI API](docs/en/api-reference/openai-api.md) |

---

### 🤖 Supported Providers

| Provider               | Status     | Supported Models             | Compatible APIs |
| ---------------------- | ---------- | ---------------------------- | --------------- |
| **OpenAI**             | ✅ Done    | GPT-4, GPT-4o, GPT-5, etc.   | OpenAI, Anthropic, Gemini, Embedding, Image Generation |
| **Anthropic**          | ✅ Done    | Claude 3.5, Claude 3.0, etc. | OpenAI, Anthropic, Gemini |
| **Zhipu AI**           | ✅ Done    | GLM-4.5, GLM-4.5-air, etc.   | OpenAI, Anthropic, Gemini |
| **Moonshot AI (Kimi)** | ✅ Done    | kimi-k2, etc.                | OpenAI, Anthropic, Gemini |
| **DeepSeek**           | ✅ Done    | DeepSeek-V3.1, etc.          | OpenAI, Anthropic, Gemini |
| **ByteDance Doubao**   | ✅ Done    | doubao-1.6, etc.             | OpenAI, Anthropic, Gemini, Image Generation |
| **Gemini**             | ✅ Done    | Gemini 2.5, etc.             | OpenAI, Anthropic, Gemini, Image Generation |
| **Fireworks**          | ✅ Done    | MiniMax-M2.5, GLM-5, Kimi K2.5, etc. | OpenAI |
| **Jina AI**            | ✅ Done    | Embeddings, Reranker, etc.   | Jina Embedding, Jina Rerank |
| **OpenRouter**         | ✅ Done    | Various models               | OpenAI, Anthropic, Gemini, Image Generation |
| **ZAI**                | ✅ Done    | -                            | Image Generation |
| **AWS Bedrock**        | 🔄 Testing | Claude on AWS                | OpenAI, Anthropic, Gemini |
| **Google Cloud**       | 🔄 Testing | Claude on GCP                | OpenAI, Anthropic, Gemini |
| **NanoGPT**            | ✅ Done    | Various models, Image Gen    | OpenAI, Anthropic, Gemini, Image Generation |

---

## 🚀 Quick Start

### 30-Second Local Start

```bash
# Download and extract (macOS ARM64 example)
curl -sSL https://github.com/looplj/axonhub/releases/latest/download/axonhub_darwin_arm64.tar.gz | tar xz
cd axonhub_*

# Run with SQLite (default)
./axonhub

# Open http://localhost:8090
# First run: Follow the setup wizard to initialize the system (create admin account, password must be at least 6 characters)
```

That's it! Now configure your first AI channel and start calling models through AxonHub.

### Rust Deployment Artifacts

The Rust backend is deployed through the following canonical artifacts:

- Release assets named `axonhub-rust_<tag>_<platform>.(tar.gz|zip)`
- Docker images `ghcr.io/looplj/axonhub:rust-latest` and `ghcr.io/looplj/axonhub:rust-<tag>`
- Compose example at `docker-compose.rust.yml`

These artifacts provide the Rust CLI/config contract and ship the verified SQLite- and PostgreSQL-backed surface covered by the Rust test suite: `/health`, admin bootstrap/status, identity/request-context, admin read routes, the full `/admin/graphql` subset (settings, user management, project/role management, quota, operational mutations), OpenAPI GraphQL, and the complete inference families including `/v1/images/generations`, `/v1/images/edits`, `/v1/images/variations`, `/v1/realtime` (JSON POST and WebSocket upgrade with session management), video generation, and all provider-specific routes (Jina, Anthropic, Gemini, Doubao). AiSDK compatibility and provider-edge OAuth are also fully supported.

### Zero-Code Migration Example

**Your existing code works without any changes.** Just point your SDK to AxonHub:

```python
from openai import OpenAI

client = OpenAI(
    base_url="http://localhost:8090/v1",  # Point to AxonHub
    api_key="your-axonhub-api-key"        # Use AxonHub API key
)

# Call Claude using OpenAI SDK!
response = client.chat.completions.create(
    model="claude-3-5-sonnet",  # Or gpt-4, gemini-pro, deepseek-chat...
    messages=[{"role": "user", "content": "Hello!"}]
)
```

Switch models by changing one line: `model="gpt-4"` → `model="claude-3-5-sonnet"`. No SDK changes needed.

### 1-click Deploy to Render

Deploy AxonHub with 1-click on [Render](https://render.com) for free.

<div>

<a href="https://render.com/deploy?repo=https://github.com/looplj/axonhub">
  <img src="https://render.com/images/deploy-to-render-button.svg" alt="Deploy to Render">
</a>

</div>

---

## 🚀 Deployment Guide

### 💻 Personal Computer Deployment

Perfect for individual developers and small teams. No complex configuration required.

#### Quick Download & Run

1. **Download the latest release** from [GitHub Releases](https://github.com/looplj/axonhub/releases)

   - Choose the appropriate version for your operating system:

2. **Extract and run**

   ```bash
   # Extract the downloaded file
   unzip axonhub_*.zip
   cd axonhub_*

   # Add execution permissions (only for Linux/macOS)
   chmod +x axonhub

   # Run directly - default SQLite database

   # Install AxonHub to system
   sudo ./install.sh

   # Start AxonHub service
   ./start.sh

   # Stop AxonHub service
   ./stop.sh
   ```

3. **Access the application**
   ```
   http://localhost:8090
   ```

---

### 🖥️ Server Deployment

For production environments, high availability, and enterprise deployments.

> **Important:** The Rust backend is verified for **SQLite and PostgreSQL**. MySQL is not part of the Rust target-state support contract in this repository. TiDB and Neon DB remain documented in the legacy Go tree as historical reference only, not as the canonical deployment path. See the [Backend Architecture](#backend-architecture) section for details.

#### Database Support

**Rust Backend (Verified Support):**

| Database | Supported Versions | Recommended Scenario | Auto Migration | Links |
| -------- | ------------------ | -------------------- | -------------- | ------ |
| **SQLite** | 3.0+ | Development, small deployments | ✅ Supported | [SQLite](https://www.sqlite.org/index.html) |
| **PostgreSQL** | 15+ | Production environment, medium-large deployments | ✅ Supported | [PostgreSQL](https://www.postgresql.org/) |

**Historical Reference Only (legacy Go contract material):**

| Database | Supported Versions | Recommended Scenario | Auto Migration | Links |
| -------- | ------------------ | -------------------- | -------------- | ------ |
| **TiDB Cloud** | Starter | Serverless, Free tier, Auto Scale | ✅ Supported (Go only) | [TiDB Cloud](https://www.pingcap.com/tidb-cloud-starter/) |
| **TiDB Cloud** | Dedicated | Distributed deployment, large scale | ✅ Supported (Go only) | [TiDB Cloud](https://www.pingcap.com/tidb-cloud-dedicated/) |
| **TiDB** | V8.0+ | Distributed deployment, large scale | ✅ Supported (Go only) | [TiDB](https://tidb.io/) |
| **Neon DB** | - | Serverless, Free tier, Auto Scale | ✅ Supported (Go only) | [Neon DB](https://neon.com/) |

These entries are preserved as historical reference for the legacy Go contract surface. The canonical deployment guidance in this repository stays on the Rust backend.

#### Configuration

**Rust Backend (SQLite default; PostgreSQL verified):**

AxonHub uses YAML configuration files with environment variable override support. The Rust backend uses SQLite by default with no additional configuration needed. PostgreSQL uses the same verified `db.dialect` / `db.dsn` contract.

```yaml
# config.yml
server:
  port: 8090
  name: "AxonHub"
  debug: false

# SQLite is the default database for the Rust backend.
# No additional db configuration needed - data is stored in ./axonhub.db
```

Environment variables (optional):

```bash
AXONHUB_SERVER_PORT=8090
AXONHUB_LOG_LEVEL=info
# SQLite needs no extra DB config because it's the default
# PostgreSQL example:
# AXONHUB_DB_DIALECT=postgres
# AXONHUB_DB_DSN=postgres://user:pass@localhost/axonhub?sslmode=disable
```

**Historical Reference Only (legacy Go dialect examples):**

TiDB and Neon DB examples remain in the legacy Go tree and documentation as reference material only. The canonical deployment guidance in this repository stays on the Rust backend and its explicitly documented supported surface.


#### Docker Compose Deployment

**Rust Backend (SQLite):**

Use the provided `docker-compose.rust.yml` file for the Rust backend with SQLite:

```bash
# Start Rust backend with SQLite
docker-compose -f docker-compose.rust.yml up -d

# Check status
docker-compose -f docker-compose.rust.yml ps
```

**Historical Reference Only (legacy Go dialect examples):**

TiDB and Neon DB compose examples remain in the legacy Go tree and documentation as reference material only. This deployment guide keeps Rust as the canonical operator path.

#### Helm Kubernetes Deployment

Deploy AxonHub on Kubernetes using the official Helm chart. This deployment path uses the Rust backend as the canonical image and targets PostgreSQL as the verified Kubernetes database path. TiDB and Neon DB references remain historical material in the legacy Go tree; this Helm path documents the Rust canonical image only.

The Helm chart is the recommended Kubernetes deployment method for the Rust backend.

```bash
# Quick installation
git clone https://github.com/looplj/axonhub.git
cd axonhub
helm install axonhub ./deploy/helm

# Production deployment
helm install axonhub ./deploy/helm -f ./deploy/helm/values-production.yaml

# Access AxonHub
kubectl port-forward svc/axonhub 8090:8090
# Visit http://localhost:8090
```

**Key Configuration Options:**

| Parameter | Description | Default |
|-----------|-------------|---------|
| `axonhub.replicaCount` | Replicas | `1` |
| `axonhub.dbPassword` | DB password | `axonhub_password` |
| `postgresql.enabled` | Embedded PostgreSQL | `true` |
| `ingress.enabled` | Enable ingress | `false` |
| `persistence.enabled` | Data persistence | `false` |

For detailed configuration and troubleshooting, see [Helm Chart Documentation](deploy/helm/README.md).

#### Virtual Machine Deployment

**Rust Backend (SQLite):**

Download the Rust-specific release from [GitHub Releases](https://github.com/looplj/axonhub/releases) (look for `axonhub-rust_*` assets).

```bash
# Extract and run
unzip axonhub-rust_*.zip
cd axonhub-rust_*

# Install
sudo ./install.sh

# Configuration file check (optional)
axonhub config validate

# Start service
./start.sh

# Stop service
./stop.sh
```

**Historical Reference Only (legacy Go dialect examples):**

TiDB and Neon DB virtual-machine examples remain documented in the legacy Go tree as historical reference only. This README keeps Rust-tagged assets as the canonical release path.

---

## 📖 Usage Guide

### Unified API Overview

AxonHub provides a unified API gateway that supports both OpenAI Chat Completions and Anthropic Messages APIs. This means you can:

- **Use OpenAI API to call Anthropic models** - Keep using your OpenAI SDK while accessing Claude models
- **Use Anthropic API to call OpenAI models** - Use Anthropic's native API format with GPT models
- **Use Gemini API to call OpenAI models** - Use Gemini's native API format with GPT models
- **Automatic API translation** - AxonHub handles format conversion automatically
- **Zero code changes** - Your existing OpenAI or Anthropic client code continues to work

### 1. Initial Setup

1. **Access Management Interface**

   ```
   http://localhost:8090
   ```

2. **Configure AI Providers**

   - Add API keys in the management interface
   - Test connections to ensure correct configuration

3. **Create Users and Roles**
   - Set up permission management
   - Assign appropriate access permissions

### 2. Channel Configuration

Configure AI provider channels in the management interface. For detailed information on channel configuration, including model mappings, parameter overrides, and troubleshooting, see the [Channel Configuration Guide](docs/en/guides/channel-management.md).

### 3. Model Management

AxonHub provides a flexible model management system that supports mapping abstract models to specific channels and model implementations through Model Associations. This enables:

- **Unified Model Interface** - Use abstract model IDs (e.g., `gpt-4`, `claude-3-opus`) instead of channel-specific names
- **Intelligent Channel Selection** - Automatically route requests to optimal channels based on association rules and load balancing
- **Flexible Mapping Strategies** - Support for precise channel-model matching, regex patterns, and tag-based selection
- **Priority-based Fallback** - Configure multiple associations with priorities for automatic failover

For comprehensive information on model management, including association types, configuration examples, and best practices, see the [Model Management Guide](docs/en/guides/model-management.md).

### 4. Create API Keys

Create API keys to authenticate your applications with AxonHub. Each API key can be configured with multiple profiles that define:

- **Model Mappings** - Transform user-requested models to actual available models using exact match or regex patterns
- **Channel Restrictions** - Limit which channels an API key can use by channel IDs or tags
- **Model Access Control** - Control which models are accessible through a specific profile
- **Profile Switching** - Change behavior on-the-fly by activating different profiles

For detailed information on API key profiles, including configuration examples, validation rules, and best practices, see the [API Key Profile Guide](docs/en/guides/api-key-profiles.md).

### 5. AI Coding Tools Integration

See the dedicated guides for detailed setup steps, troubleshooting, and tips on combining these tools with AxonHub model profiles:
- [OpenCode Integration Guide](docs/en/guides/opencode-integration.md)
- [Claude Code Integration Guide](docs/en/guides/claude-code-integration.md)
- [Codex Integration Guide](docs/en/guides/codex-integration.md)

---

### 6. SDK Usage

For detailed SDK usage examples and code samples, please refer to the API documentation:
- [OpenAI API](docs/en/api-reference/openai-api.md)
- [Anthropic API](docs/en/api-reference/anthropic-api.md)
- [Gemini API](docs/en/api-reference/gemini-api.md)

## 🛠️ Development Guide

For detailed development instructions, architecture design, and contribution guidelines, please see [docs/en/development/development.md](docs/en/development/development.md).

---

## 🤝 Acknowledgments

- 🙏 [musistudio/llms](https://github.com/musistudio/llms) - LLM transformation framework, source of inspiration
- 🎨 [satnaing/shadcn-admin](https://github.com/satnaing/shadcn-admin) - Admin interface template
- 🔧 [99designs/gqlgen](https://github.com/99designs/gqlgen) - GraphQL code generation for the legacy backend
- 🌐 [gin-gonic/gin](https://github.com/gin-gonic/gin) - HTTP framework for the legacy backend
- 🗄️ [ent/ent](https://github.com/ent/ent) - ORM framework for the legacy backend
- 🦀 [tokio-rs/axum](https://github.com/tokio-rs/axum) - HTTP framework for the Rust backend (early phase)
- ⚙️ [actix-rs/actix-web](https://github.com/actix-rs/actix-web) - HTTP framework for the Rust backend (production)
- ⚙️ [tokio-rs/tokio](https://github.com/tokio-rs/tokio) - Async runtime for the Rust backend
- ☁️ [Render](https://render.com) - Free cloud deployment platform for hosting our demo
- 🗃️ [TiDB Cloud](https://www.pingcap.com/tidb-cloud/) - Serverless database platform for demo deployment

---

## 📄 License

This project is licensed under multiple licenses (Apache-2.0 and LGPL-3.0). See [LICENSE](LICENSE) file for the detailed licensing overview and terms.

---

<div align="center">

**AxonHub** - All-in-one AI Development Platform, making AI development simpler

[🏠 Homepage](https://github.com/looplj/axonhub) • [📚 Documentation](https://deepwiki.com/looplj/axonhub) • [🐛 Issue Feedback](https://github.com/looplj/axonhub/issues)

Built with ❤️ by the AxonHub team

</div>
