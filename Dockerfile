# syntax=docker/dockerfile:1

# Shared runtime: deployment policy lives here, regardless of build input.
FROM alpine:3.23 AS runtime
RUN apk add --no-cache ca-certificates \
    && mkdir -p /data /app/web \
    && chown 65532:65532 /data \
    && chmod 700 /data
ENV BRAINS_DATA_DIR=/data \
    BRAINS_BIND=0.0.0.0 \
    BRAINS_PORT=45800 \
    BRAINS_WEB_DIST=/app/web
WORKDIR /app
USER 65532:65532
EXPOSE 45800
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD wget -q -O /dev/null "http://127.0.0.1:${BRAINS_PORT}/healthz" || exit 1
ENTRYPOINT ["/usr/local/bin/brains-server"]

# CI/release reuses the exact binaries and web assets already built and tested.
FROM runtime AS release
COPY --chmod=755 docker-artifacts/brains-server /usr/local/bin/brains-server
COPY docker-artifacts/web/ /app/web/

# Local source builds use the same runtime without needing a host Rust/Node toolchain.
FROM rust:1.96-alpine3.23 AS server-build
RUN apk add --no-cache build-base
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/
COPY migrations/ migrations/
COPY apps/ apps/
COPY sdk/rust/ sdk/rust/
ARG CARGO_BUILD_JOBS=2
ARG BRAINS_BUILD_GIT_SHA
ARG BRAINS_BUILD_GIT_DIRTY
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    if [ -n "$BRAINS_BUILD_GIT_SHA" ]; then export BRAINS_BUILD_GIT_SHA; else unset BRAINS_BUILD_GIT_SHA; fi; \
    if [ -n "$BRAINS_BUILD_GIT_DIRTY" ]; then export BRAINS_BUILD_GIT_DIRTY; else unset BRAINS_BUILD_GIT_DIRTY; fi; \
    cargo build --locked --release -p brains-server && cp target/release/brains-server /brains-server

FROM node:22-alpine AS web-build
WORKDIR /src
RUN corepack enable
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY apps/web/package.json apps/web/package.json
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile --filter brains-web...
COPY apps/web/ apps/web/
COPY sdk/typescript/ sdk/typescript/
RUN pnpm --dir apps/web build

FROM runtime AS source
COPY --from=server-build /brains-server /usr/local/bin/brains-server
COPY --from=web-build /src/apps/web/dist/ /app/web/
