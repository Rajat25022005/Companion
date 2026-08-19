# -----------------------------------------------------------------------------
# Stage 1: Build & Compile (Rust 1.84+ Debian Slim)
# -----------------------------------------------------------------------------
FROM rust:1.84-slim-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace manifests
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY services ./services
COPY bins ./bins
COPY migrations ./migrations

# Build release binaries
RUN cargo build --release --bin companion-api --bin companion

# -----------------------------------------------------------------------------
# Stage 2: Runtime Minimal Image
# -----------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

WORKDIR /app

# Install runtime SSL & certs
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy compiled binaries from builder
COPY --from=builder /app/target/release/companion-api /usr/local/bin/companion-api
COPY --from=builder /app/target/release/companion /usr/local/bin/companion
COPY --from=builder /app/migrations /app/migrations

# Configuration defaults
ENV PORT=8000
ENV RUST_LOG=companion=info,tower_http=info
EXPOSE 8000

# Healthcheck probe
HEALTHCHECK --interval=10s --timeout=3s --retries=3 \
  CMD curl -f http://localhost:8000/v1/ready || exit 1

ENTRYPOINT ["companion-api"]
