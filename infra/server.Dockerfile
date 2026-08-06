FROM node:22-bookworm-slim AS web-builder
WORKDIR /workspace
COPY package.json package-lock.json ./
RUN npm ci
COPY app ./app
COPY components ./components
COPY lib ./lib
COPY next.config.ts tsconfig.json ./
RUN npm run build

FROM rust:1-bookworm AS server-builder
WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY server ./server
RUN cargo build --release --package langai-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /var/lib/langai/audio /opt/langai/web \
    && chown -R 65532:65532 /var/lib/langai /opt/langai
COPY --from=server-builder /workspace/target/release/langai-server /usr/local/bin/langai-server
COPY --from=web-builder /workspace/out /opt/langai/web
USER 65532:65532
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/langai-server"]
