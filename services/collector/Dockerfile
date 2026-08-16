# syntax=docker/dockerfile:1
# SuperOps Collector 采集/治理服务镜像（多阶段构建）
# 构建：docker build -f services/collector/Dockerfile -t superops/collector:1.8.0 .
FROM rust:1.88-slim AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev protobuf-compiler && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release -p superops-collector

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -u 10001 superops
# 治理备份依赖本机 mysqldump（housekeeping 任务）；镜像内随包提供
RUN apt-get update && apt-get install -y --no-install-recommends default-mysql-client \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/superops-collector /usr/local/bin/superops-collector
USER superops
ENV COLLECTOR_CONFIG=/etc/superops/collector.yaml
CMD ["superops-collector"]
