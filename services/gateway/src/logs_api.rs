use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use ecat_data::RdbmsClient;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct LogSearchQuery {
    pub namespace: Option<String>,
    pub pod: Option<String>,
    pub keyword: Option<String>,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub limit: Option<i64>,
}

fn clamp_limit(q: &LogSearchQuery) -> i64 {
    q.limit.unwrap_or(100).clamp(1, 1000)
}

fn escape_like(s: &str) -> String {
    s.replace('\'', "''")
}

fn build_where(q: &LogSearchQuery) -> Vec<String> {
    let mut w = Vec::new();
    if let Some(ns) = &q.namespace
        && !ns.is_empty()
    {
        w.push(format!("namespace = '{}'", escape_like(ns)));
    }
    if let Some(p) = &q.pod
        && !p.is_empty()
    {
        w.push(format!("pod = '{}'", escape_like(p)));
    }
    if let Some(k) = &q.keyword
        && !k.is_empty()
    {
        let k = k.replace('%', "");
        w.push(format!("content ILIKE '%{}%'", escape_like(&k)));
    }
    if let Some(f) = q.from {
        w.push(format!("timestamp >= {f}"));
    }
    if let Some(t) = q.to {
        w.push(format!("timestamp <= {t}"));
    }
    w
}

fn fmt_ts(unix: i64) -> String {
    chrono::DateTime::from_timestamp(unix, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

pub async fn search_logs(
    State(state): State<crate::AppState>,
    Query(q): Query<LogSearchQuery>,
) -> impl IntoResponse {
    let limit = clamp_limit(&q);
    // 日志检索后端：search 段配置时走 ES/OpenSearch，否则走 ClickHouse pod_log
    if let Some(search) = &state.search {
        let mut must: Vec<serde_json::Value> = Vec::new();
        if let Some(ns) = q.namespace.as_deref().filter(|s| !s.is_empty()) {
            must.push(serde_json::json!({ "term": { "namespace": ns } }));
        }
        if let Some(p) = q.pod.as_deref().filter(|s| !s.is_empty()) {
            must.push(serde_json::json!({ "term": { "pod": p } }));
        }
        if let Some(k) = q.keyword.as_deref().filter(|s| !s.is_empty()) {
            must.push(serde_json::json!({ "match": { "content": k } }));
        }
        let mut range = serde_json::Map::new();
        if let Some(f) = q.from {
            range.insert("gte".into(), serde_json::json!(f));
        }
        if let Some(t) = q.to {
            range.insert("lte".into(), serde_json::json!(t));
        }
        if !range.is_empty() {
            must.push(serde_json::json!({ "range": { "timestamp": range } }));
        }
        let query = serde_json::json!({
            "query": { "bool": { "must": must } },
            "sort": [{ "timestamp": { "order": "desc" } }],
            "size": limit,
        });
        return match search.search(&state.search_index, &query).await {
            Ok(resp) => {
                let logs: Vec<serde_json::Value> = resp
                    .pointer("/hits/hits")
                    .and_then(serde_json::Value::as_array)
                    .map(|hits| {
                        hits.iter()
                            .filter_map(|h| h.get("_source").cloned())
                            .map(|mut src| {
                                let ts = src
                                    .get("timestamp")
                                    .and_then(serde_json::Value::as_i64)
                                    .map(fmt_ts)
                                    .unwrap_or_default();
                                if let Some(o) = src.as_object_mut() {
                                    o.insert("ts".into(), serde_json::json!(ts));
                                }
                                src
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (StatusCode::OK, Json(serde_json::json!({ "logs": logs }))).into_response()
            }
            Err(e) => (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("log search failed: {e}") })),
            )
                .into_response(),
        };
    }
    let where_sql = build_where(&q);
    let sql = if where_sql.is_empty() {
        format!(
            "SELECT namespace, pod, content, formatDateTime(toDateTime(timestamp), '%Y-%m-%d %H:%i:%s') AS ts FROM pod_log ORDER BY timestamp DESC LIMIT {limit}"
        )
    } else {
        format!(
            "SELECT namespace, pod, content, formatDateTime(toDateTime(timestamp), '%Y-%m-%d %H:%i:%s') AS ts FROM pod_log WHERE {} ORDER BY timestamp DESC LIMIT {limit}",
            where_sql.join(" AND ")
        )
    };
    match state.ch.query(&sql).await {
        Ok(rows) => {
            let logs: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    let mut m = serde_json::Map::new();
                    for col in ["namespace", "pod", "content", "ts"] {
                        if let Some(v) = r.get(col) {
                            m.insert(col.to_string(), v.clone());
                        }
                    }
                    serde_json::Value::Object(m)
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({ "logs": logs }))).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("log query failed: {e}") })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn where_clause_escapes_and_combines() {
        let q = LogSearchQuery {
            namespace: Some("default".into()),
            keyword: Some("error%' OR 1=1".into()),
            from: Some(1700000000),
            ..Default::default()
        };
        let w = build_where(&q);
        // exact equality pins the security properties: quote doubled (injection
        // inert inside the literal), user-supplied '%' stripped, LIKE pattern intact
        assert!(w.iter().any(|s| s == "namespace = 'default'"));
        assert!(w.iter().any(|s| s == "content ILIKE '%error'' OR 1=1%'"));
        assert!(w.iter().any(|s| s == "timestamp >= 1700000000"));
    }

    #[test]
    fn limit_is_clamped() {
        assert_eq!(
            clamp_limit(&LogSearchQuery {
                limit: Some(99999),
                ..Default::default()
            }),
            1000
        );
        assert_eq!(clamp_limit(&LogSearchQuery::default()), 100);
    }
}
