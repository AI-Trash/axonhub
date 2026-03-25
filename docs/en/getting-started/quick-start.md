# Quick Start Guide

## Before You Start

AxonHub is currently in an additive Go-to-Rust backend migration.

- If you want the **full product experience**, use Docker or released binaries.
- If you want to work on the **Rust migration slice**, use the Cargo workspace in this repository or the dedicated Rust-tagged release assets and Docker image.

The Rust slice already preserves config loading, CLI shape, `/health`, the verified SQLite- and PostgreSQL-backed bootstrap/system routes, the migrated OpenAI-compatible practical `/v1` subset, and explicit `501` responses for unported route families. The same SeaORM-backed slice is wired for MySQL but is not yet fully integration-verified, and TiDB/Neon remain Go-only.

## Prerequisites

- Docker and Docker Compose for the full local product experience
- Or Rust 1.78+, Go 1.26+, Node.js 18+, and pnpm for repository development
- A valid API key from an AI provider

## Fastest Path: Full Local Runtime

### 1. Clone the repository

```bash
git clone https://github.com/looplj/axonhub.git
cd axonhub
```

### 2. Prepare configuration

```bash
cp config.example.yml config.yml
```

### 3. Start the stack

```bash
docker-compose up -d
```

### 4. Open AxonHub

- Web interface: `http://localhost:8090`

## Rust Migration Slice Quick Start

If you are working on the new Rust backend slice:

```bash
cargo run -p axonhub-server -- help
cargo run -p axonhub-server -- config preview
cargo run -p axonhub-server -- config validate
cargo run -p axonhub-server -- build-info
```

You can also pull the published Rust migration-slice image directly:

```bash
docker run --rm -p 8090:8090 ghcr.io/looplj/axonhub:rust-latest
```

That image is best for quickly validating the Rust slice. `/health` is the immediate readiness check; the bootstrap/system routes plus the migrated OpenAI-compatible practical `/v1` subset remain limited to the verified SQLite- and PostgreSQL-backed migration paths rather than a fresh full-product setup. The same SeaORM-backed slice is wired for MySQL but is not yet fully integration-verified, and TiDB/Neon remain on the Go backend.

What to expect from the Rust slice right now:

- `/health` works
- `GET /admin/system/status` and `POST /admin/system/initialize` work for the supported SQLite- and PostgreSQL-backed migration paths
- `/v1/models`, `/v1/chat/completions`, `/v1/responses`, and `/v1/embeddings` work on the migrated practical SQLite- and PostgreSQL-backed paths
- MySQL uses the same SeaORM-backed repository seam, but full Rust-side integration verification is still pending
- TiDB and Neon DB remain Go-only
- config search paths and `AXONHUB_*` env keys are supported
- unported route families return structured `501 Not Implemented` JSON

## First Product Steps

Once the full backend is running, the normal AxonHub onboarding flow remains the same:

1. configure your first provider channel,
2. create an API key,
3. point your SDK at AxonHub,
4. start routing requests through the unified API.

## Example API Usage

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

## What the Migration Changes

The migration changes how the backend is implemented, not what AxonHub aims to provide.

- Product docs still describe the full AxonHub feature set.
- The Rust workspace is the new implementation path.
- Until more route families are ported, the Go backend remains the complete runtime.

## Related Documentation

- [Configuration Guide](../deployment/configuration.md)
- [Docker Deployment](../deployment/docker.md)
- [Development Guide](../development/development.md)
