# SuperOps — Intelligent Ops Platform

An intelligent operations platform built on the [e-cat](https://github.com/erik/e-cat) framework ecosystem (v1.5.0). An API gateway provides auth, rate limiting, circuit breaking and proxying; a Kubernetes resource service provides queries, logs, Watch and terminal exec; a Collector handles metric snapshots, inspection and alerts; a Tauri desktop frontend completes the picture.

## Overview

SuperOps targets small-to-medium infrastructure operations with a closed loop from "seeing" to "acting":

- **See**: cluster resources (Pod/Deployment/Node), pod logs, live terminals, metric snapshots and alerts
- **Resilience**: rate limiting (Redis shared counters + Consul dynamic thresholds), circuit breaking (auto half-open after consecutive 5xx), auth (JWT / OAuth2 / API Key)
- **Observability**: Prometheus metrics, OTLP tracing (Jaeger), ClickHouse time-series storage
- **Coordination**: Consul registration/discovery + remote config, Kafka audit event bus

Four phases (all complete): P1 MVP (auth + resource queries) → P2 resilience & observability (circuit breaker, rate limit, registry, tracing) → P3 integration & API surface (exec terminal, API Keys, OAuth2, OpenAPI) → P4–P6 operations loop (P4 write ops / audit / user management → P5 logs / CMDB / scripts / RBAC → P6 governance / approvals / multi-tenancy / vault / recordings / files / alerts / metrics / Helm).

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
│   │       ├── cmdb_api.rs / logs_api.rs / scripts_api.rs #  log search / CMDB / scripts
│   │       ├── approval_api.rs #   approval workflow (delete gate)
│   │       ├── secrets_api.rs / vault.rs # secret vault (AES-256-GCM, 503 without master key)
│   │       ├── recordings_api.rs / recorder.rs # terminal recordings (ClickHouse exec_session)
│   │       ├── files_api.rs   #    file upload/download (10MB multipart, MinIO)
│   │       ├── alerts_api.rs  #    alert center (list/ack)
│   │       └── openapi.rs     #    /api/docs
│   ├── k8s/                   #  K8s resource service :9091 (gRPC)
│   │   └── src/
│   │       ├── cluster/       #    cluster management (kubeconfig add/remove/get)
│   │       ├── resource/      #    pod/deploy/node/exec/metrics + job (RunJob) queries
│   │       └── service.rs     #    tonic service (log stream / Watch / exec bidi + RunJob)
│   └── collector/             #  metric collection, inspection & log collection
│       └── src/
│           ├── collect.rs     #    periodic collection → ClickHouse snapshots
│           ├── logtail.rs     #    Pod log collection → ClickHouse
│           ├── inspect.rs     #    inspection (Redis distributed lock)
│           ├── alert.rs       #    alert rules & event writes
│           ├── housekeeping.rs #   governance: MySQL backup / capacity & cost estimates
│           └── events.rs      #    Kafka audit event consumption
├── frontend/                  # React + Vite + Tauri 2 (:3000)
│   └── src/pages/            #  k8s (clusters/Pods/Nodes/terminal/logs) + cmdb + ops (audit/API Keys/users/scripts/alerts/metrics/log search/recordings/approvals/secrets/files)
├── ecat-*/                    # e-cat framework crates (workspace members)
├── superops-protos/           # generated protobuf code (common.v1 / k8s.v1)
├── protos/                    # proto sources (managed by buf)
├── bench/                     # ecat-bench load-test entry (login)
├── config/                    # per-service YAML configs
├── deploy/                    # docker-compose.yml + init.sql + prometheus.yml + helm/superops (Chart)
└── docs/                      # audit report / plans / architecture diagram
```

![Project structure](docs/images/tree.svg)

## Features

### Gateway (:8080)
| Feature | Description |
|---|---|
| Auth | login/register; JWT sign/verify via framework ecat-auth (HS256, unified `AuthClaims` injection, secret ≥32 bytes enforced); optional OAuth2 layer (ecat-auth, enabled by adding an `oauth2` config block); `X-API-Key` in-memory store (instant revocation); query-token fallback for WS; role system admin/operator/viewer (first registered user becomes admin; operator gets api:read/api:write/ops:audit/ops:cmdb/ops:scripts, viewer is api:read-only; API Keys resolve through the same role mapping) |
| Rate limit | login 10 req / 60s / IP; Redis shared counters (falls back to memory); thresholds hot-reload via Consul KV |
| Circuit breaker | k8s proxy trips after consecutive 5xx (50% failure / 30s window / half-open probe 3), 503 while open |
| Proxy | `/api/k8s/*` → gRPC (queries + scale/restart/delete write ops); `/exec` WebSocket ↔ gRPC bidi stream (incl. terminal resize, write route require_role("api:write")); write ops publish Kafka audit events (`k8s.scale`/`k8s.restart`/`k8s.delete`, payload includes `level: "INFO"`) |
| Log search | `GET /api/logs/search`: ClickHouse log search (ingested by collector logtail), require_role("api:read") |
| CMDB assets | `/api/cmdb/assets` (GET list / POST create), `/api/cmdb/assets/{id}` (DELETE), `/api/cmdb/stats` (GET), require_role("ops:cmdb") |
| Script library | `/api/scripts` (GET/POST), `/api/scripts/{id}` (DELETE), `/api/scripts/{id}/run` (POST → k8s Job batch execution), `/api/scripts/runs` (GET run history), shell only (busybox:1.36), require_role("ops:scripts") |
| Approvals | deletion gate for deployments (412 without an approved request when `approval.enabled`); `GET/POST /api/approvals` (kind=delete, target `"{cluster_id}/{ns}/{name}"`), `POST /api/approvals/{id}/decide`; **off by default** |
| Multi-tenancy | `x-tenant-id` header parsed by `require_tenant` middleware (lowercase alnum/dash 1..=64, falls back to `default`); cmdb/scripts data filtered by `tenant_id` |
| Vault | `/api/secrets` (GET list / POST create / GET+DELETE single, AES-256-GCM); all endpoints return 503 without a valid `SUPEROPS_MASTER_KEY` (32 bytes); ciphertext never logged |
| Recordings | exec WebSocket frames mirrored to ClickHouse `exec_session` (side channel; frames carry a per-session seq, replay ordered by timestamp, seq); `/api/recordings` (GET list), `/api/recordings/{sid}/frames` (GET, up to 5000 frames), `/api/recordings/{sid}` (DELETE); read api:read / write api:write; `recording.enabled` config switch (on by default) |
| Files | `POST /api/files` (multipart, 10MB limit, filename whitelist) + `GET /api/files/{name}`, stored in MinIO, api:write/api:read |
| Alert center | `GET /api/alerts?level=&limit=50` (ClickHouse `alert_event`, ops:cmdb), `POST /api/alerts/{id}/ack` (api:write, idempotent), `GET /api/alerts/acks` |
| Metrics page | frontend `/ops/metrics` reuses `GET /api/v1/metrics/query` (api:read) |
| Governance | collector `housekeeping`: MySQL backup (mysqldump → `data/backups`, keep N), disk capacity & cost estimates (collector.yaml `housekeeping:` block) |
| Observability | `/health`, `/ready` (MySQL dependency check), `/metrics` (Prometheus), `/api/docs` (OpenAPI 3.0.3), OTLP spans (gateway/k8s/collector) |
| Ops | Consul registration + `discover("superops-k8s")` endpoint resolution (static fallback); KV hot reload |

### K8s Service (:9091)
Cluster management (kubeconfig), Pod/Deployment/Node queries (Node incl. Ready status & kubelet version), Deployment scale/restart/delete write ops (gRPC `ScaleDeployment`/`RestartDeployment`/`DeleteDeployment`), batch execution (gRPC `RunJob` → batch/v1 Job, shell scripts), pod log streams (tail/follow), resource Watch (ADDED/MODIFIED/DELETED), exec bidi stream (stdin/stdout/stderr + resize).

### Collector
Periodic metric collection → ClickHouse snapshots; log collection (logtail.rs: periodic Pod log ingestion → ClickHouse); inspection guarded by a Redis lock (single instance); alert rules (consecutive over-threshold) write events; alert notifications (generic/DingTalk/WeCom webhooks + per-target silence window, default 300s); governance (housekeeping.rs: MySQL backup + capacity/cost estimates); Kafka audit event consumption (ecat-mq-kafka); OTLP span export.

### Frontend (:3000)
Login, cluster list/detail, pod list & logs, deployments, nodes, terminal page (WS bidi, query-token auth, binaryType handling, optional container); dashboard cards wired to real data (cluster count / Docker host count (metrics query, temporary `app` metric) / active alerts (wired to the alert center API since P6)); CMDB assets page (`/cmdb`, ProTable + create dialog, menu between Kubernetes and Ops Center); script library page (`/ops/scripts`, dual ProTable for scripts + run history); ops center (`/ops/audit` audit log, `/ops/apikeys` API Keys, `/ops/users` user management, `/ops/alerts` alert center (10s polling + ack), `/ops/metrics` metrics dashboard (reuses the metrics query API), `/ops/logs` log search (filters + time range), `/ops/recordings` terminal recordings (frames ordered by seq, playback), `/ops/approvals` approval center (status filter + approve/reject/cancel/reopen), `/ops/secrets` secret vault (encrypted storage + decrypted view/copy, 503 notice when master key unset), `/ops/files` file manager (multipart upload / download by name)); standalone Pods/Deployments/Nodes pages now carry a cluster picker (first cluster by default, no longer hardcoded to `default`; embedded pages in the cluster detail view are unaffected).

### API surface (OpenAPI at /api/docs)
`/api/auth/register|login`, `/api/keys` (CRUD), `/api/users` (`PATCH /{id}/status` enable/disable), `/api/audit/events` (audit query, limit/offset/level), `/api/logs/search` (log search), `/api/cmdb/assets` (GET/POST), `/api/cmdb/stats` (GET), `/api/cmdb/assets/{id}` (DELETE), `/api/scripts` (GET/POST), `/api/scripts/{id}` (DELETE), `/api/scripts/{id}/run` (POST), `/api/scripts/runs` (GET), `/api/approvals` (GET/POST, delete-approval gate), `/api/approvals/{id}/decide` (POST), `/api/secrets` (GET/POST), `/api/secrets/{name}` (GET/DELETE), `/api/recordings` (GET), `/api/recordings/{sid}` (DELETE), `/api/recordings/{sid}/frames` (GET), `/api/files` (POST upload), `/api/files/{name}` (GET download), `/api/alerts` (GET, level/limit), `/api/alerts/{id}/ack` (POST), `/api/alerts/acks` (GET), `/api/k8s/clusters[/{id}][/pods|/deployments|/nodes|/metrics]`, `/api/k8s/clusters/{id}/pods/{ns}/{pod}/logs|exec`, `/api/k8s/clusters/{id}/deployments/{ns}/{name}/scale|restart` (`DELETE` to remove), `/api/v1/metrics/query`, `/api/health`, `/api/docs`.

## Quick Start

```bash
# 1. Infrastructure (MySQL 3307 / Redis 6380 / ClickHouse 8124 / Prometheus 9095 /
#    Kafka 9092 / Consul 8500 / Jaeger 4317+16686 / MinIO 9002+9003)
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
| gateway gRPC | 9090 |
| k8s service gRPC | 9091 |
| collector | 0 (tasks only) |
| frontend (vite) | 3000 |
| MinIO | 9002 (API) / 9003 (console) |
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
| `SUPEROPS_MASTER_KEY` | vault master key (32 bytes; `/api/secrets` returns 503 when unset) |
| `MINIO_ROOT_USER` / `MINIO_ROOT_PASSWORD` | MinIO credentials (`deploy/.env`, change in production) |
| `RUST_LOG` | tracing level (defaults to info) |

Infra passwords are injected via `deploy/.env` (template: `deploy/.env.example`); service YAMLs live in `config/`.

## Tests & CI

- 361 workspace unit/integration tests, all passing (gateway 73 / k8s 14 / collector 32 / ecat crates; see the test matrix in `docs/audit-report-2026-08-06.md`)
- E2E & security: login/register, k8s route happy & error paths, auth bypass, JWT forgery, 429 rate limit, 503 breaker, API Key lifecycle (see `docs/audit-report-2026-08-06.md`)
- Runtime checks: Consul register/deregister, KV hot reload (threshold 3↔10 both ways), Jaeger spans, Prometheus target up
- CI (`.github/workflows/ci.yml`): `cargo fmt --check` + `cargo check` + `cargo test` + `npm run build`

## Known Limits

- **OAuth2**: off by default; requires an `oauth2` config block (introspection URL + client id/secret) and an external IdP
- **Master key**: `SUPEROPS_MASTER_KEY` unset by default (all `/api/secrets` endpoints return 503 when unset or not 32 bytes; two-state availability); must be set before enabling the vault in production
- **Approval switch**: `approval.enabled` is off by default (`config/gateway.yaml`); when enabled, deleting a deployment requires a submitted and approved delete request (412 gate)
- **exec terminal**: needs a real k8s cluster and role operator+ (write route require_role("api:write")); without a cluster, clients get an explicit error frame
- **OTLP coverage**: gateway HTTP + k8s gRPC + collector scheduler spans are wired; non-blocking side paths (e.g. recording mirror) carry no resource field (accepted, non-blocking)
- **Multi-tenancy**: tenant is asserted from the client-supplied `x-tenant-id` header (no server-side tenant directory); missing header falls back to `default`, present-but-malformed returns 400 (strict mode)
- **Recording switch**: `recording.enabled` is on by default (`config/gateway.yaml`); replay is capped at 5000 frames (ORDER BY timestamp, seq)
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
