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
    if let Some(ns) = &q.namespace {
        if !ns.is_empty() {
            w.push(format!("namespace = '{}'", escape_like(ns)));
        }
    }
    if let Some(p) = &q.pod {
        if !p.is_empty() {
            w.push(format!("pod = '{}'", escape_like(p)));
        }
    }
    if let Some(k) = &q.keyword {
        if !k.is_empty() {
            let k = k.replace('%', "");
            w.push(format!("content ILIKE '%{}%'", escape_like(&k)));
        }
    }
    if let Some(f) = q.from {
        w.push(format!("timestamp >= {f}"));
    }
    if let Some(t) = q.to {
        w.push(format!("timestamp <= {t}"));
    }
    w
}

pub async fn search_logs(
    State(state): State<crate::AppState>,
    Query(q): Query<LogSearchQuery>,
) -> impl IntoResponse {
    let limit = clamp_limit(&q);
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
