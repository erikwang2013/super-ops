use crate::AppState;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use ecat_data::RdbmsClient;
use redis::AsyncCommands;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const ACK_TTL_SECS: u64 = 3600;
const ACK_TTL: Duration = Duration::from_secs(ACK_TTL_SECS);

#[derive(Debug, Deserialize)]
pub struct AlertsQuery {
    pub level: Option<String>,
    pub limit: Option<i64>,
}

/// 查询参数白名单 → 存储字面量。collector 写入的是大写 CRIT/WARN（alert.rs），
/// 这里同时兼容小写别名，全部来自固定常量表（无用户输入进入 SQL）。
const LEVEL_ALIASES: [(&str, &[&str]); 3] = [
    ("info", &["INFO", "info"]),
    ("warning", &["WARN", "warning"]),
    ("critical", &["CRIT", "critical"]),
];

fn clamp_limit(v: Option<i64>) -> i64 {
    v.unwrap_or(50).clamp(1, 500)
}

fn level_cond(level: &str) -> Option<String> {
    LEVEL_ALIASES
        .iter()
        .find(|(k, _)| *k == level)
        .map(|(_, vals)| {
            let quoted: Vec<String> = vals.iter().map(|v| format!("'{v}'")).collect();
            format!("level IN ({})", quoted.join(", "))
        })
}

pub async fn list_alerts(
    State(state): State<AppState>,
    Query(q): Query<AlertsQuery>,
) -> impl IntoResponse {
    let limit = clamp_limit(q.limit);
    // alert_event 无自然主键：以写入时间戳（秒）作为 id（collector now_secs()），
    // 同一秒内的多条告警共享一个 id，ack 时整组置灰——与前端 10s 轮询粒度可接受。
    let mut sql = "SELECT level, title, message, timestamp AS id FROM alert_event".to_string();
    if let Some(level) = &q.level {
        let Some(cond) = level_cond(level) else {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "level must be one of: info, warning, critical"})),
            )
                .into_response();
        };
        sql.push_str(" WHERE ");
        sql.push_str(&cond);
    }
    sql.push_str(&format!(" ORDER BY timestamp DESC LIMIT {limit}"));
    match state.ch.query(&sql).await {
        Ok(rows) => {
            let alerts: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    let mut m = serde_json::Map::new();
                    for col in ["id", "level", "title", "message"] {
                        if let Some(v) = r.get(col) {
                            m.insert(col.to_string(), v.clone());
                        }
                    }
                    serde_json::Value::Object(m)
                })
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({ "alerts": alerts })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("alert query failed: {e}") })),
        )
            .into_response(),
    }
}

/// 告警确认存储：优先 Redis SETEX `alert:ack:{id}`（TTL 1h），
/// 连接失败或运行时不可用时回退内存 HashMap<u64, Instant>（读取时裁剪过期项）。
fn merge_ids(redis_ids: Vec<u64>, mem_ids: Vec<u64>) -> Vec<u64> {
    let mut all = redis_ids;
    all.extend(mem_ids);
    all.sort_unstable();
    all.dedup();
    all
}

pub struct AlertAckStore {
    redis: Option<redis::aio::MultiplexedConnection>,
    mem: Mutex<HashMap<u64, Instant>>,
}

impl AlertAckStore {
    pub async fn connect(url: &str) -> Self {
        let redis = match redis::Client::open(url) {
            Ok(client) => match client.get_multiplexed_async_connection().await {
                Ok(conn) => {
                    tracing::info!("alert ack store connected to redis");
                    Some(conn)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "alert ack redis unavailable; falling back to in-memory");
                    None
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "alert ack redis url invalid; falling back to in-memory");
                None
            }
        };
        Self {
            redis,
            mem: Mutex::new(HashMap::new()),
        }
    }

    #[cfg(test)]
    fn memory() -> Self {
        Self {
            redis: None,
            mem: Mutex::new(HashMap::new()),
        }
    }

    pub async fn ack(&self, id: u64) {
        let key = format!("alert:ack:{id}");
        if let Some(conn) = &self.redis {
            let mut conn = conn.clone();
            match conn.set_ex::<_, _, ()>(&key, "1", ACK_TTL_SECS).await {
                Ok(()) => return,
                Err(e) => {
                    tracing::warn!(error = %e, "alert ack redis set failed; falling back to in-memory")
                }
            }
        }
        self.mem
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, Instant::now());
    }

    pub async fn acked_ids(&self) -> Vec<u64> {
        if let Some(conn) = &self.redis {
            let mut conn = conn.clone();
            let mut iter: redis::AsyncIter<String> = match conn.scan_match("alert:ack:*").await {
                Ok(it) => it,
                Err(e) => {
                    tracing::warn!(error = %e, "alert ack redis scan failed; falling back to in-memory");
                    return self.mem_ids();
                }
            };
            let mut ids = Vec::new();
            while let Some(key) = iter.next_item().await {
                if let Some(id) = key
                    .strip_prefix("alert:ack:")
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    ids.push(id);
                }
            }
            // ack 写入 Redis 失败时已落到内存，此处合并，避免短暂故障后 ack 丢失
            return merge_ids(ids, self.mem_ids());
        }
        self.mem_ids()
    }

    fn mem_ids(&self) -> Vec<u64> {
        let mut mem = self.mem.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        mem.retain(|_, at| now.duration_since(*at) < ACK_TTL);
        let mut ids: Vec<u64> = mem.keys().copied().collect();
        ids.sort_unstable();
        ids
    }
}

pub async fn ack_alert(State(state): State<AppState>, Path(id): Path<u64>) -> impl IntoResponse {
    state.alert_acks.ack(id).await;
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

pub async fn list_acks(State(state): State<AppState>) -> impl IntoResponse {
    let ids = state.alert_acks.acked_ids().await;
    (StatusCode::OK, Json(serde_json::json!({ "ids": ids }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_cond_whitelists_and_maps() {
        assert_eq!(
            level_cond("critical").as_deref(),
            Some("level IN ('CRIT', 'critical')")
        );
        assert_eq!(
            level_cond("warning").as_deref(),
            Some("level IN ('WARN', 'warning')")
        );
        assert_eq!(
            level_cond("info").as_deref(),
            Some("level IN ('INFO', 'info')")
        );
        assert_eq!(level_cond("fatal"), None);
        assert_eq!(level_cond("'), DROP TABLE alert_event--"), None);
    }

    #[test]
    fn limit_is_clamped() {
        assert_eq!(clamp_limit(None), 50);
        assert_eq!(clamp_limit(Some(9999)), 500);
        assert_eq!(clamp_limit(Some(0)), 1);
    }

    #[tokio::test]
    async fn mem_store_acks_and_prunes() {
        let store = AlertAckStore::memory();
        store.ack(1700000001).await;
        store.ack(1700000002).await;
        assert_eq!(store.acked_ids().await, vec![1700000001, 1700000002]);
        // 过期条目在读取时被裁剪
        store.mem.lock().unwrap().insert(
            1700000000,
            Instant::now() - ACK_TTL - Duration::from_secs(1),
        );
        assert_eq!(store.acked_ids().await, vec![1700000001, 1700000002]);
    }

    // Redis 故障期间的 ack 落入内存；redis 恢复后（scan 走 redis）仍须合并内存中的 id
    #[test]
    fn merge_ids_unions_redis_and_memory() {
        assert_eq!(merge_ids(vec![1, 3], vec![2, 3]), vec![1, 2, 3]);
        assert_eq!(merge_ids(vec![], vec![5]), vec![5]);
        assert_eq!(merge_ids(vec![1], vec![]), vec![1]);
    }
}
