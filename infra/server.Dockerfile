FROM rust:1-bookworm AS builder
WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY server ./server
RUN cargo build --release --package langai-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /workspace/target/release/langai-server /usr/local/bin/langai-server
USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/langai-server"]
