# bench

SuperOps 负载压测 crate（包名 `bench`，不是 `superops-bench`）。

依赖网关已启动。默认打 `GET /api/health`；可用环境变量切换。

```bash
cargo run -p bench
BENCH_TARGET=login cargo run -p bench
BENCH_GATEWAY_URL=http://localhost:8080/api/health cargo run -p bench
```

| 变量 | 默认 | 说明 |
|------|------|------|
| `BENCH_TARGET` | `health` | `health` 或 `login` |
| `BENCH_GATEWAY_URL` | health → `/api/health`；login → `/api/auth/login` | 覆盖目标 URL |

CI `bench-smoke` 只执行 `cargo build -p bench`（编译冒烟，不连真实服务）。
