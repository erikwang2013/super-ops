use crate::AppState;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use ecat_data::TsdbClient;
use serde::Deserialize;
use serde_json::Value;

/// 与 collector housekeeping.rs 对齐的估算单价（元/核·小时、元/GiB·小时）
pub const CPU_PRICE: f64 = 0.5;
pub const MEM_PRICE: f64 = 0.25;

pub fn estimate_cost(cpu_cores: f64, mem_gib: f64) -> f64 {
    (cpu_cores * CPU_PRICE + mem_gib * MEM_PRICE) * 24.0
}

fn num(v: &Value, key: &str) -> f64 {
    v.get(key)
        .and_then(|x| x.as_f64())
        .or_else(|| v.get(key).and_then(|x| x.as_i64()).map(|i| i as f64))
        .unwrap_or(0.0)
}

#[derive(Debug, Deserialize)]
pub struct TrendQuery {
    pub hours: Option<i64>,
}

/// 汇总：近 24h 最新容量快照 + 成本估算（元/天、元/月）。
pub async fn capacity_summary_handler(State(state): State<AppState>) -> impl IntoResponse {
    let sql = "SELECT argMax(cpu_cores, timestamp) AS cpu_cores, \
               argMax(mem_gib, timestamp) AS mem_gib, \
               argMax(node_count, timestamp) AS node_count \
               FROM capacity_snapshot WHERE timestamp > now() - 86400";
    let value = match TsdbClient::query(state.ch.as_ref(), sql).await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("capacity query failed: {e}") })),
            )
                .into_response();
        }
    };
    let row = value
        .as_array()
        .and_then(|a| a.first())
        .cloned()
        .unwrap_or_default();
    let cpu = num(&row, "cpu_cores");
    let mem = num(&row, "mem_gib");
    let nodes = num(&row, "node_count") as i64;
    let cost_day = estimate_cost(cpu, mem);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "cpu_cores": cpu,
            "mem_gib": mem,
            "node_count": nodes,
            "cost_yuan_day": (cost_day * 100.0).round() / 100.0,
            "cost_yuan_month": ((cost_day * 30.0) * 100.0).round() / 100.0,
        })),
    )
        .into_response()
}

/// 趋势：按小时桶聚合各集群最新容量并求和（子查询先取 (bucket,cluster) 最新值）。
pub async fn capacity_trend_handler(
    State(state): State<AppState>,
    Query(q): Query<TrendQuery>,
) -> impl IntoResponse {
    let hours = q.hours.unwrap_or(24).clamp(1, 168);
    let sql = format!(
        "SELECT bucket, sum(cpu_cores) AS cpu, sum(mem_gib) AS mem, sum(node_count) AS nodes \
         FROM ( \
           SELECT toStartOfInterval(timestamp, INTERVAL 1 HOUR) AS bucket, cluster, \
                  argMax(cpu_cores, timestamp) AS cpu_cores, \
                  argMax(mem_gib, timestamp) AS mem_gib, \
                  argMax(node_count, timestamp) AS node_count \
           FROM capacity_snapshot WHERE timestamp > now() - INTERVAL {hours} HOUR \
           GROUP BY bucket, cluster \
         ) GROUP BY bucket ORDER BY bucket"
    );
    let value = match TsdbClient::query(state.ch.as_ref(), &sql).await {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("capacity query failed: {e}") })),
            )
                .into_response();
        }
    };
    let rows: Vec<Value> = value.as_array().cloned().unwrap_or_default();
    let points: Vec<Value> = rows
        .iter()
        .map(|r| {
            let cpu = num(r, "cpu");
            let mem = num(r, "mem");
            serde_json::json!({
                "bucket": r.get("bucket").cloned().unwrap_or_default(),
                "cpu_cores": cpu,
                "mem_gib": mem,
                "node_count": num(r, "nodes") as i64,
                "cost_yuan_day": (estimate_cost(cpu, mem) * 100.0).round() / 100.0,
            })
        })
        .collect();
    (
        StatusCode::OK,
        Json(serde_json::json!({ "points": points })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_estimate_matches_formula() {
        // 8 核 + 16 GiB： (8*0.5 + 16*0.25) * 24 = 8*24 = 192 元/天
        assert_eq!(estimate_cost(8.0, 16.0), 192.0);
    }

    #[test]
    fn num_extracts_int_and_float() {
        assert_eq!(num(&serde_json::json!({"a": 3}), "a"), 3.0);
        assert_eq!(num(&serde_json::json!({"a": 3.5}), "a"), 3.5);
        assert_eq!(num(&serde_json::json!({"a": "x"}), "a"), 0.0);
        assert_eq!(num(&serde_json::json!({"b": 1}), "a"), 0.0);
    }
}
