# Quick Start Guide

## Before You Start

AxonHub's canonical backend in this repository is Rust.

- If you want the fastest local runtime for the currently supported product surface, use the Rust Docker/compose path or the Rust-tagged release assets.
- If you want to work on backend code in this repository, use the Cargo workspace; the legacy Go tree remains in-repo only as historical reference/oracle material.

The Rust backend already preserves config loading, CLI shape, `/health`, the verified SQLite- and PostgreSQL-backed bootstrap/system routes, the current OpenAI-compatible practical `/v1` surface, and explicit `501` responses for the accepted unsupported route families. SQLite and PostgreSQL are the Rust target-state databases in this repository, and TiDB/Neon remain legacy-reference dialect material in the Go tree.

## Prerequisites

- Docker and Docker Compose for the fastest local runtime path
- Or Rust 1.78+, Go 1.26+, Node.js 18+, and pnpm for repository development
- A valid API key from an AI provider

## Fastest Path: Canonical Local Runtime

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
docker-compose -f docker-compose.rust.yml up -d
```

### 4. Open AxonHub

- Web interface: `http://localhost:8090`

## Rust Backend Quick Start

If you are working on the Rust backend in this repository:

```bash
cargo run -p axonhub-server -- help
cargo run -p axonhub-server -- config preview
cargo run -p axonhub-server -- config validate
cargo run -p axonhub-server -- build-info
```

You can also pull the published Rust image directly:

```bash
docker run --rm -p 8090:8090 ghcr.io/looplj/axonhub:rust-latest
```

That image is best for quickly validating the current Rust-supported surface. `/health` is the immediate readiness check; the bootstrap/system routes plus the practical OpenAI-compatible `/v1` surface remain limited to the verified SQLite- and PostgreSQL-backed Rust backend paths. TiDB/Neon remain legacy-reference dialect material in the Go tree.

What to expect from the Rust backend right now:

- `/health` works
- `GET /admin/system/status` and `POST /admin/system/initialize` work for the supported SQLite- and PostgreSQL-backed Rust paths
- `/v1/models`, `/v1/chat/completions`, `/v1/responses`, and `/v1/embeddings` work on the current practical SQLite- and PostgreSQL-backed Rust paths
- TiDB and Neon DB remain legacy-reference dialect material in the Go tree
- config search paths and `AXONHUB_*` env keys are supported
- accepted explicit unsupported route families return structured `501 Not Implemented` JSON

## First Product Steps

Once the Rust backend is running, the normal AxonHub onboarding flow remains the same:

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

## What This Means for the Backend

The backend implementation is now centered on the Rust workspace and Rust-tagged artifacts, not on maintaining a dual-primary backend posture.

- Product docs still describe the full AxonHub feature set.
- The Rust workspace and Rust-tagged artifacts are the canonical implementation path.
- Accepted explicit unsupported boundaries stay explicit until additional Rust verification lands.
- The legacy Go tree remains in-repo only as historical reference/oracle material.

## Related Documentation

- [Configuration Guide](../deployment/configuration.md)
- [Docker Deployment](../deployment/docker.md)
- [Development Guide](../development/development.md)
