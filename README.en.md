# SuperOps — Intelligent Ops Platform

An intelligent operations platform built on the [e-cat](https://github.com/erik/e-cat) framework ecosystem (v1.2.0). An API gateway provides auth, rate limiting, circuit breaking and proxying; a Kubernetes resource service provides queries, logs, Watch and terminal exec; a Collector handles metric snapshots, inspection and alerts; a Tauri desktop frontend completes the picture.

## Overview

SuperOps targets small-to-medium infrastructure operations with a closed loop from "seeing" to "acting":

- **See**: cluster resources (Pod/Deployment/Node), pod logs, live terminals, metric snapshots and alerts
- **Resilience**: rate limiting (Redis shared counters + Consul dynamic thresholds), circuit breaking (auto half-open after consecutive 5xx), auth (JWT / OAuth2 / API Key)
- **Observability**: Prometheus metrics, OTLP tracing (Jaeger), ClickHouse time-series storage
- **Coordination**: Consul registration/discovery + remote config, Kafka audit event bus

Four phases: P1 MVP (auth + resource queries) → P2 resilience & observability (circuit breaker, rate limit, registry, tracing) → P3 integration & API surface (exec terminal, API Keys, OAuth2, OpenAPI) → P0 infra completion (full Compose stack).

## Architecture

```
┌────────────┐   HTTP/WS  ┌─────────────┐   gRPC   ┌────────────┐
│  Frontend  │ ─────────► │   Gateway   │ ───────► │  K8s svc   │ ──► Kubernetes API
│ Tauri/Web  │   :8080    │ auth+limit  │  :9091   │   :9091    │
└────────────┘            └──────┬──────┘          └────────────┘
                                 │ gRPC
                          ┌──────▼──────┐
                          │  Collector  │────► ClickHouse (snapshots/alerts)
                          └──────┬──────┘
                                 │
     MySQL (users/keys) Redis (rate limit/lock) Kafka (audit) Consul (registry/KV) Prometheus/Jaeger
```

![Architecture](docs/images/architecture.svg)

**Request path**: browser → Gateway (auth → rate limit → breaker → proxy) → K8s service (gRPC) → Kubernetes API; terminals are bridged WebSocket ↔ gRPC bidi stream. **Config path**: Consul KV `config/superops/gateway/*` changes are pushed in real time via blocking queries — `rate.limit.max`/`rate.limit.window` apply immediately. **Observability path**: Gateway exposes `/metrics` (scraped by Prometheus) and OTLP spans (shown in Jaeger).

### Diagram Gallery

![Request flow](docs/images/flow.svg)

![Layered design](docs/images/design.svg)

![Functional structure](docs/images/structure.svg)

![Security defenses](docs/images/security.svg)

![Service lifecycle](docs/images/lifecycle.svg)

## Project Layout

```
super-ops/
├── services/                  # backend services (one binary each)
│   ├── gateway/               #  API gateway :8080
│   │   └── src/
│   │       ├── auth/          #    JWT issue/verify (ecat-auth), API Keys, OAuth2 short-circuit, login/register
│   │       ├── proxy/         #    k8s reverse proxy (HTTP queries + WS terminal bridge)
│   │       ├── breaker.rs     #    circuit breaker (FiveXx→Error chain)
│   │       ├── config_remote.rs #  Consul KV hot reload (DynamicRateLimitStore)
│   │       ├── metrics*.rs    #    Prometheus export + metrics query API
│   │       └── openapi.rs     #    /api/docs
│   ├── k8s/                   #  K8s resource service :9091 (gRPC)
│   │   └── src/
│   │       ├── cluster/       #    cluster management (kubeconfig add/remove/get)
│   │       ├── resource/      #    pod/deploy/node/exec/metrics queries
│   │       └── service.rs     #    tonic service (log stream / Watch / exec bidi)
│   └── collector/             #  metric collection & inspection
│       └── src/
│           ├── collect.rs     #    periodic collection → ClickHouse snapshots
│           ├── inspect.rs     #    inspection (Redis distributed lock)
│           ├── alert.rs       #    alert rules & event writes
│           └── events.rs      #    Kafka audit event consumption
├── frontend/                  # React + Vite + Tauri 2 (:3000)
│   └── src/pages/k8s/         #  clusters/Pods/Deployments/Nodes/terminal/logs
├── ecat-*/                    # e-cat framework crates (workspace members)
├── superops-protos/           # generated protobuf code (common.v1 / k8s.v1)
├── protos/                    # proto sources (managed by buf)
├── bench/                     # ecat-bench load-test entry (login)
├── config/                    # per-service YAML configs
├── deploy/                    # docker-compose.yml + init.sql + prometheus.yml
└── docs/                      # audit report / plans / architecture diagram
```

![Project structure](docs/images/tree.svg)

## Features

### Gateway (:8080)
| Feature | Description |
|---|---|
| Auth | login/register; JWT sign/verify via framework ecat-auth (HS256, unified `AuthClaims` injection, secret ≥32 bytes enforced); optional OAuth2 layer (ecat-auth, enabled by adding an `oauth2` config block); `X-API-Key` in-memory store (instant revocation); query-token fallback for WS |
| Rate limit | login 10 req / 60s / IP; Redis shared counters (falls back to memory); thresholds hot-reload via Consul KV |
| Circuit breaker | k8s proxy trips after consecutive 5xx (50% failure / 30s window / half-open probe 3), 503 while open |
| Proxy | `/api/k8s/*` → gRPC; `/exec` WebSocket ↔ gRPC bidi stream (incl. terminal resize) |
| Observability | `/health`, `/ready` (MySQL dependency check), `/metrics` (Prometheus), `/api/docs` (OpenAPI 3.0.3), OTLP spans |
| Ops | Consul registration + `discover("superops-k8s")` endpoint resolution (static fallback); KV hot reload |

### K8s Service (:9091)
Cluster management (kubeconfig), Pod/Deployment/Node queries (Node incl. Ready status & kubelet version), pod log streams (tail/follow), resource Watch (ADDED/MODIFIED/DELETED), exec bidi stream (stdin/stdout/stderr + resize).

### Collector
Periodic metric collection → ClickHouse snapshots; inspection guarded by a Redis lock (single instance); alert rules (consecutive over-threshold) write events; Kafka audit event consumption (ecat-mq-kafka).

### Frontend (:3000)
Login, cluster list/detail, pod list & logs, deployments, nodes, terminal page (WS bidi, query-token auth, binaryType handling, optional container).

### API surface (OpenAPI at /api/docs)
`/api/auth/register|login`, `/api/keys` (CRUD), `/api/k8s/clusters[/{id}][/pods|/deployments|/nodes|/metrics]`, `/api/k8s/clusters/{id}/pods/{ns}/{pod}/logs|exec`, `/api/v1/metrics/query`, `/api/health`, `/api/docs`.

## Quick Start

```bash
# 1. Infrastructure (MySQL 3307 / Redis 6380 / ClickHouse 8124 / Prometheus 9095 /
#    Kafka 9092 / Consul 8500 / Jaeger 4317+16686)
cd deploy && cp .env.example .env && sudo docker compose up -d

# 2. Generate protobuf (first time or after proto changes)
make proto

# 3. Backends (three services)
make dev          # cargo build gateway + k8s-service + collector
cd services/gateway && GATEWAY_CONFIG=../../config/gateway.yaml cargo run &
cd services/k8s && K8S_CONFIG=../../config/k8s-service.yaml cargo run &
cd services/collector && COLLECTOR_CONFIG=../../config/collector.yaml cargo run &

# 4. Frontend
cd frontend && npm install && npm run dev    # http://localhost:3000
```

Or one-shot: `make dev-all` (compose + backends + frontend).

Test account (local dev DB): `erik / Test1234!`

## Ports

| Component | Port |
|---|---|
| gateway HTTP | 8080 |
| k8s service gRPC | 9091 |
| frontend (vite) | 3000 |
| MySQL | 3307 |
| Redis | 6380 |
| ClickHouse | 8124 (HTTP) / 9001 (native) |
| Prometheus | 9095 |
| Kafka | 9092 |
| Consul | 8500 |
| Jaeger | 4317 (OTLP) / 16686 (UI) |

## Configuration

| Env var | Purpose |
|---|---|
| `GATEWAY_CONFIG` / `K8S_CONFIG` / `COLLECTOR_CONFIG` | per-service config YAML path |
| `SUPEROPS_JWT_SECRET` | JWT signing key (production: ≥32 random bytes; warns and falls back if unset) |
| `RUST_LOG` | tracing level (defaults to info) |

Infra passwords are injected via `deploy/.env` (template: `deploy/.env.example`); service YAMLs live in `config/`.

## Tests & CI

- 296 workspace unit/integration tests (gateway / k8s / collector / ecat crates)
- E2E & security: login/register, k8s route happy & error paths, auth bypass, JWT forgery, 429 rate limit, 503 breaker, API Key lifecycle (see `docs/audit-report-2026-08-06.md`)
- Runtime checks: Consul register/deregister, KV hot reload (threshold 3↔10 both ways), Jaeger spans, Prometheus target up
- CI (`.github/workflows/ci.yml`): `cargo fmt --check` + `cargo check` + `cargo test` + `npm run build`

## Known Limits

- **OAuth2**: off by default; requires an `oauth2` config block (introspection URL + client id/secret) and an external IdP
- **exec terminal**: needs a real k8s cluster; without one, clients get an explicit error frame
- **gRPC-layer spans**: OTLP tracing covers the HTTP path (gateway); k8s/collector gRPC/scheduler spans are not yet wired
- **Single-instance deployment**: rate-limit counters are shared via Redis, but services themselves run as single instances

## Docs

- `docs/audit-report-2026-08-06.md` — full audit report (test matrix, security checklist, fixes)
- `docs/images/` — architecture & diagram gallery (architecture / flow / design / structure / security / lifecycle)
- `docs/superpowers/plans/` — per-phase implementation plans (P1 MVP / P2 resilience / P3 integration)
- `CHANGELOG.md` — changelog

## Support

If this project helps you, we welcome your support — scan the QR code to donate (Alipay / WeChat Pay):

| Alipay (支付宝) | WeChat Pay (微信) |
|---|---|
| <img src="docs/alipay.png" width="130" height="130" alt="Alipay QR code"> | <img src="docs/weixinpay.png" width="130" height="130" alt="WeChat Pay QR code"> |
