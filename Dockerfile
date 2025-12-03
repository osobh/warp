# Multi-stage Dockerfile for warp (CPU-only)
# Optimized for minimal image size using Debian bookworm

# ============================================================================
# Build Stage: Compile Rust binaries
# ============================================================================
FROM rust:1.85-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy workspace configuration first for better layer caching
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY .cargo ./.cargo

# Copy all crate manifests to establish dependency graph
COPY crates/warp-cli/Cargo.toml ./crates/warp-cli/
COPY crates/warp-core/Cargo.toml ./crates/warp-core/
COPY crates/warp-format/Cargo.toml ./crates/warp-format/
COPY crates/warp-net/Cargo.toml ./crates/warp-net/
COPY crates/warp-compress/Cargo.toml ./crates/warp-compress/
COPY crates/warp-hash/Cargo.toml ./crates/warp-hash/
COPY crates/warp-crypto/Cargo.toml ./crates/warp-crypto/
COPY crates/warp-io/Cargo.toml ./crates/warp-io/
COPY crates/warp-gpu/Cargo.toml ./crates/warp-gpu/
COPY crates/warp-stream/Cargo.toml ./crates/warp-stream/
COPY crates/portal-core/Cargo.toml ./crates/portal-core/
COPY crates/portal-hub/Cargo.toml ./crates/portal-hub/
COPY crates/portal-net/Cargo.toml ./crates/portal-net/
COPY crates/warp-edge/Cargo.toml ./crates/warp-edge/
COPY crates/warp-sched/Cargo.toml ./crates/warp-sched/
COPY crates/warp-orch/Cargo.toml ./crates/warp-orch/
COPY crates/warp-telemetry/Cargo.toml ./crates/warp-telemetry/
COPY crates/warp-config/Cargo.toml ./crates/warp-config/

# Create dummy source files to cache dependencies
RUN mkdir -p crates/warp-cli/src && echo "fn main() {}" > crates/warp-cli/src/main.rs && \
    for crate in warp-core warp-format warp-net warp-compress warp-hash warp-crypto \
                 warp-io warp-gpu warp-stream portal-core portal-hub portal-net \
                 warp-edge warp-sched warp-orch warp-telemetry warp-config; do \
        mkdir -p crates/$crate/src && echo "" > crates/$crate/src/lib.rs; \
    done

# Build dependencies only (this layer will be cached)
RUN cargo build --release --bin warp && \
    rm -rf target/release/.fingerprint/warp-* && \
    rm -rf target/release/deps/warp_* && \
    find target/release -name "warp*" -type f -delete

# Copy actual source code
COPY crates ./crates

# Build the actual binary with release optimizations
RUN cargo build --release --bin warp

# Verify binary exists and works
RUN /app/target/release/warp --version

# ============================================================================
# Runtime Stage: Minimal runtime environment
# ============================================================================
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN useradd -m -u 1000 -s /bin/bash warp

# Copy binary from builder
COPY --from=builder /app/target/release/warp /usr/local/bin/warp

# Set ownership
RUN chown warp:warp /usr/local/bin/warp

# Create data directory
RUN mkdir -p /data && chown warp:warp /data

# Switch to non-root user
USER warp
WORKDIR /data

# Expose default port
EXPOSE 9999

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD warp --version || exit 1

# Default entrypoint
ENTRYPOINT ["warp"]
CMD ["--help"]
