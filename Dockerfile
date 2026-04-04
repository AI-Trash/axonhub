ARG RUST_MUSL_IMAGE_AMD64=ghcr.io/blackdex/rust-musl:x86_64-musl-stable
ARG RUST_MUSL_IMAGE_ARM64=ghcr.io/blackdex/rust-musl:aarch64-musl-stable

FROM ${RUST_MUSL_IMAGE_AMD64} AS rust-musl-amd64
FROM ${RUST_MUSL_IMAGE_ARM64} AS rust-musl-arm64

ARG TARGETARCH=amd64
FROM rust-musl-${TARGETARCH} AS builder

ARG TARGETARCH
ARG AXONHUB_BUILD_COMMIT=""
ARG AXONHUB_BUILD_TIME=""
ARG AXONHUB_BUILD_RUST_VERSION=""

WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY apps ./apps
COPY crates ./crates
COPY internal/build/VERSION ./internal/build/VERSION

RUN case "${TARGETARCH}" in \
      amd64) RUST_TARGET="x86_64-unknown-linux-musl" ;; \
      arm64) RUST_TARGET="aarch64-unknown-linux-musl" ;; \
      *) echo "Unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
    && export AXONHUB_BUILD_COMMIT="${AXONHUB_BUILD_COMMIT}" \
    && export AXONHUB_BUILD_TIME="${AXONHUB_BUILD_TIME}" \
    && export AXONHUB_BUILD_RUST_VERSION="${AXONHUB_BUILD_RUST_VERSION:-$(rustc --version)}" \
    && cargo build --locked --release --target "${RUST_TARGET}" -p axonhub-server \
    && cp "target/${RUST_TARGET}/release/axonhub" /tmp/axonhub

FROM alpine:latest

RUN apk add --no-cache ca-certificates libgcc tzdata wget

WORKDIR /app

COPY --from=builder /tmp/axonhub /usr/local/bin/axonhub

EXPOSE 8090

ENTRYPOINT ["/usr/local/bin/axonhub"]
