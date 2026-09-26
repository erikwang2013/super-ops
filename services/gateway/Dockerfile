# syntax=docker/dockerfile:1
# SuperOps API 网关镜像（多阶段构建）
# 构建：docker build -f services/gateway/Dockerfile -t superops/gateway:dev .
FROM rust:1.98.1-slim AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev protobuf-compiler build-essential && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release -p superops-gateway

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -u 10001 superops
COPY --from=builder /build/target/release/superops-gateway /usr/local/bin/superops-gateway
USER superops
EXPOSE 8080
# 配置经 GATEWAY_CONFIG 注入（Helm 以 ConfigMap 挂载）；生产须设 SUPEROPS_JWT_SECRET / SUPEROPS_MASTER_KEY
ENV GATEWAY_CONFIG=/etc/superops/gateway.yaml
CMD ["superops-gateway"]
