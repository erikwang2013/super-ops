# syntax=docker/dockerfile:1
# SuperOps K8s 资源服务镜像（多阶段构建）
# 构建：docker build -f services/k8s/Dockerfile -t superops/k8s:dev .
FROM rust:1.88-slim AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev protobuf-compiler build-essential && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release -p superops-k8s

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -u 10001 superops
COPY --from=builder /build/target/release/superops-k8s /usr/local/bin/superops-k8s
USER superops
EXPOSE 9091
ENV K8S_CONFIG=/etc/superops/k8s-service.yaml
CMD ["superops-k8s"]
