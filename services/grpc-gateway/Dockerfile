# Build stage
FROM rust:1.83-bookworm AS builder

WORKDIR /app

# Install protobuf compiler
RUN apt-get update && apt-get install -y protobuf-compiler && rm -rf /var/lib/apt/lists/*

# Copy proto file first
COPY proto/adsb.proto /app/proto/

# Copy Cargo files
COPY services/grpc-gateway/Cargo.toml /app/
COPY services/grpc-gateway/build.rs /app/

# Fix proto path in build.rs for Docker context
RUN sed -i 's|../../proto|/app/proto|g' /app/build.rs

# Create dummy source to cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release 2>/dev/null || true

# Copy actual source
COPY services/grpc-gateway/src /app/src

# Build the application
RUN touch src/main.rs && cargo build --release

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/grpc-gateway .

# Copy frontend static files
COPY frontend /app/static

ENV GRPC_PORT=50051
ENV WS_PORT=8888
ENV DB_HOST=timescaledb
ENV DB_PORT=5432
ENV DB_NAME=adsb
ENV DB_USER=adsb
ENV DB_PASSWORD=adsb
ENV STATIC_DIR=/app/static
ENV RUST_LOG=info

EXPOSE 50051 8888

CMD ["./grpc-gateway"]
