FROM rust:stable-alpine AS builder

ARG AXONHUB_BUILD_COMMIT=""
ARG AXONHUB_BUILD_TIME=""
ARG AXONHUB_BUILD_RUST_VERSION=""

WORKDIR /build

RUN apk add --no-cache build-base pkgconfig

COPY Cargo.toml Cargo.lock ./
COPY apps ./apps
COPY crates ./crates
COPY internal/build/VERSION ./internal/build/VERSION

RUN export AXONHUB_BUILD_COMMIT="${AXONHUB_BUILD_COMMIT}" \
    && export AXONHUB_BUILD_TIME="${AXONHUB_BUILD_TIME}" \
    && export AXONHUB_BUILD_RUST_VERSION="${AXONHUB_BUILD_RUST_VERSION:-$(rustc --version)}" \
    && cargo build --locked --release -p axonhub-server

FROM alpine:latest

RUN apk add --no-cache ca-certificates libgcc tzdata wget

WORKDIR /app

COPY --from=builder /build/target/release/axonhub /usr/local/bin/axonhub

EXPOSE 8090

ENTRYPOINT ["/usr/local/bin/axonhub"]
