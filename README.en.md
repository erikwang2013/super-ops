# SuperOps — Intelligent Ops Platform

An intelligent operations platform built on the [e-cat](https://github.com) framework (Phase 1 MVP): API gateway (auth / rate limiting / proxying), Kubernetes resource service (Pod/Deployment/Node queries, logs, Watch), and a Tauri 2 desktop frontend.

## Architecture

```
┌────────────┐   HTTP  ┌─────────────┐   gRPC   ┌────────────┐
│  Frontend  │ ──────► │   Gateway   │ ───────► │  K8s service│ ──► Kubernetes API
│ Tauri/Web  │  :8080  │  auth+limit │  :9090   │   :9091    │
└────────────┘         └──────┬──────┘          └────────────┘
                              │ MySQL (users) / Redis / ClickHouse (docker)
```

- **gateway** (`services/gateway`): JWT auth middleware, login rate limiting (10 req / 60s / IP), CORS allowlist, `/api/k8s/*` route proxy
- **k8s service** (`services/k8s`): gRPC service — cluster management (kubeconfig), Pod/Deployment/Node queries, pod log streaming, resource watch
- **frontend** (`frontend`): React + Vite + Tauri 2 desktop shell, zustand state (token kept in memory only)

## Quick Start

```bash
# 1. Infrastructure (MySQL 3307 / Redis 6380 / ClickHouse 8124)
cd deploy && cp .env.example .env && docker compose up -d

# 2. Generate protobuf (first time or after proto changes)
make proto

# 3. Backends (two services)
make dev          # cargo build gateway + k8s-service
cd services/gateway && GATEWAY_CONFIG=../../config/gateway.yaml cargo run &
cd services/k8s && K8S_CONFIG=../../config/k8s-service.yaml cargo run &

# 4. Frontend
cd frontend && npm install && npm run dev    # http://localhost:3000
```

Or one-shot: `make dev-all` (compose + both backends + frontend).

Test account (local dev database): `erik / Test1234!`

## Ports

| Component | Port |
|---|---|
| gateway HTTP | 8080 |
| gateway gRPC | 9090 |
| k8s service gRPC | 9091 |
| frontend (vite) | 3000 |
| MySQL | 3307 |
| Redis | 6380 |
| ClickHouse | 8124 (HTTP) / 9001 (native) |

## Configuration

| Env var | Purpose |
|---|---|
| `GATEWAY_CONFIG` | Gateway config YAML path (default `config/gateway.yaml`) |
| `K8S_CONFIG` | K8s service config YAML path (default `config/k8s-service.yaml`) |
| `SUPEROPS_JWT_SECRET` | JWT signing secret (required in production, ≥32 random bytes; startup warns when the default is used) |

Database/Redis/ClickHouse passwords are injected into compose via `deploy/.env` (template: `deploy/.env.example`).

## Common Commands

```bash
make proto        # buf generate (protos/ → superops-protos/)
make gateway      # build gateway
make k8s-service  # build k8s service
make dev          # proto + build both backends
make dev-all      # compose + both backends + frontend
make stop-all     # stop compose + both backend processes
make clean        # cargo clean + frontend artifacts
```

## Testing & CI

- E2E: login/register + all `/api/k8s/*` routes (happy + error paths), see `docs/audit-report-2026-08-06.md`
- Security: auth bypass, JWT forgery/tampering, input validation, CORS, rate limiting (15/15 checks verified)
- CI (`.github/workflows/ci.yml`): `cargo fmt --check` + `cargo check` + `cargo test` (gateway/k8s) + `npm run build` (tsc + vite)

## Docs

- `docs/audit-report-2026-08-06.md` — full audit report (test matrix, security checklist, fix records)
- `docs/superpowers/plans/2026-08-06-super-ops-phase1-mvp.md` — Phase 1 implementation plan (44 steps)
- `CHANGELOG.md` — changelog

## License & Notes

Phase 1 MVP: single-instance deployment, in-memory rate limiting; Phase 2 roadmap: gRPC data channel (real k8s query proxying), terminal WebSocket bridge, Redis-backed shared rate limiting, httpOnly cookie sessions.
