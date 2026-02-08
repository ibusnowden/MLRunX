# =============================================================================
# MLRunX API — Standalone Dockerfile (SQLite mode)
#
# Lightweight single-container deployment with SQLite persistence.
# No external databases required.
#
# Build:
#   docker build -t mlrunx-api .
#
# Run:
#   docker run -p 3001:3001 -p 50051:50051 \
#     -v mlrunx-data:/data \
#     -e MLRUNX_API_KEY=your-secret-key \
#     mlrunx-api
# =============================================================================

# -----------------------------------------------------------------------------
# Stage 1: Build
# -----------------------------------------------------------------------------
ARG RUST_VERSION=1.88
FROM rust:${RUST_VERSION}-slim-bookworm AS builder

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy workspace manifests for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY apps/api/Cargo.toml apps/api/Cargo.toml
COPY crates/proto/Cargo.toml crates/proto/Cargo.toml
COPY crates/proto/build.rs crates/proto/build.rs
COPY services/ingest/Cargo.toml services/ingest/Cargo.toml
COPY services/processor/Cargo.toml services/processor/Cargo.toml
COPY proto proto
COPY migrations migrations

# Create dummy sources for dependency caching
RUN mkdir -p apps/api/src crates/proto/src services/ingest/src services/processor/src && \
    echo "fn main() {}" > apps/api/src/main.rs && \
    echo "pub fn dummy() {}" > crates/proto/src/lib.rs && \
    echo "fn main() {}" > services/ingest/src/main.rs && \
    echo "fn main() {}" > services/processor/src/main.rs

# Build dependencies only (cached layer)
RUN cargo build --release --bin mlrunx-api 2>/dev/null || true
RUN rm -rf apps/api/src crates/proto/src

# Copy actual source code
COPY apps/api/src apps/api/src
COPY crates/proto/src crates/proto/src

# Touch to invalidate cache
RUN touch apps/api/src/main.rs crates/proto/src/lib.rs

# Build the API binary
RUN cargo build --release --bin mlrunx-api

# -----------------------------------------------------------------------------
# Stage 2: Runtime
# -----------------------------------------------------------------------------
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create data directory for SQLite
RUN mkdir -p /data

COPY --from=builder /app/target/release/mlrunx-api /usr/local/bin/

# Default environment
ENV RUST_LOG=info,mlrunx_api=debug
ENV MLRUNX_HTTP_HOST=0.0.0.0
ENV MLRUNX_HTTP_PORT=3001
ENV MLRUNX_GRPC_PORT=50051
ENV MLRUNX_SQLITE_PATH=/data/mlrunx.db

EXPOSE 3001 50051

VOLUME /data

HEALTHCHECK --interval=10s --timeout=5s --retries=5 \
    CMD curl -f http://localhost:3001/health || exit 1

CMD ["mlrunx-api"]
