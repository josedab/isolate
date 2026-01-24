# Multi-stage Dockerfile for Isolate Server
# Supports both amd64 and arm64 architectures

# Build stage
FROM rust:1.75-bookworm AS builder

WORKDIR /app

# Install protobuf compiler for gRPC
RUN apt-get update && apt-get install -y protobuf-compiler && rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY isolate-core/Cargo.toml isolate-core/
COPY isolate-server/Cargo.toml isolate-server/
COPY isolate-cli/Cargo.toml isolate-cli/
COPY isolate-python/Cargo.toml isolate-python/

# Create dummy source files for dependency caching
RUN mkdir -p isolate-core/src isolate-server/src isolate-cli/src isolate-python/src && \
    echo "pub fn main() {}" > isolate-core/src/lib.rs && \
    echo "fn main() {}" > isolate-server/src/main.rs && \
    echo "fn main() {}" > isolate-cli/src/main.rs && \
    echo "use pyo3::prelude::*; #[pymodule] fn _isolate(_py: Python, _m: &Bound<PyModule>) -> PyResult<()> { Ok(()) }" > isolate-python/src/lib.rs

# Copy proto files
COPY proto/ proto/

# Build dependencies only (this layer is cached)
RUN cargo build --release --package isolate-server 2>/dev/null || true

# Now copy the actual source code
COPY isolate-core/src isolate-core/src
COPY isolate-server/src isolate-server/src

# Build the real application
RUN touch isolate-core/src/lib.rs isolate-server/src/main.rs && \
    cargo build --release --package isolate-server

# Runtime stage - minimal image
FROM debian:bookworm-slim AS runtime

# Install CA certificates for HTTPS and create non-root user
RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    useradd -r -s /bin/false -u 1000 isolate

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/isolate-server /app/isolate-server

# Set ownership
RUN chown -R isolate:isolate /app

# Switch to non-root user
USER isolate

# Expose gRPC port
EXPOSE 50051

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD timeout 5 bash -c '</dev/tcp/localhost/50051' || exit 1

# Default environment variables
ENV RUST_LOG=info
ENV ISOLATE_HOST=0.0.0.0
ENV ISOLATE_PORT=50051

# Run the server
ENTRYPOINT ["/app/isolate-server"]
CMD ["--host", "0.0.0.0", "--port", "50051"]
