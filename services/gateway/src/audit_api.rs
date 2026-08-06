use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use ecat_data::RdbmsClient;
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct AuditQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub level: Option<String>,
}

fn clamp_pagination(q: &AuditQuery) -> (i64, i64) {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    (limit, offset)
}

fn escape_like(s: &str) -> String {
    s.replace('\'', "''")
}

pub async fn audit_events(
    State(state): State<crate::AppState>,
    Query(q): Query<AuditQuery>,
) -> impl IntoResponse {
    let (limit, offset) = clamp_pagination(&q);
    let sql = match &q.level {
        Some(l) if !l.trim().is_empty() => format!(
            "SELECT * FROM audit_log WHERE level = '{}' ORDER BY timestamp DESC LIMIT {limit} OFFSET {offset}",
            escape_like(l)
        ),
        _ => {
            format!("SELECT * FROM audit_log ORDER BY timestamp DESC LIMIT {limit} OFFSET {offset}")
        }
    };
    match state.ch.query(&sql).await {
        Ok(rows) => {
            let events: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    let mut m = serde_json::Map::new();
                    for col in ["timestamp", "event_type", "username", "ip", "detail"] {
                        if let Some(v) = r.get(col) {
                            m.insert(col.to_string(), v.clone());
                        }
                    }
                    serde_json::Value::Object(m)
                })
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({ "events": events })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("audit query failed: {e}") })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_is_clamped() {
        assert_eq!(
            clamp_pagination(&AuditQuery {
                limit: None,
                offset: None,
                ..Default::default()
            }),
            (50, 0)
        );
        assert_eq!(
            clamp_pagination(&AuditQuery {
                limit: Some(5000),
                offset: Some(-3),
                ..Default::default()
            }),
            (500, 0)
        );
        assert_eq!(
            clamp_pagination(&AuditQuery {
                limit: Some(1),
                offset: Some(10),
                ..Default::default()
            }),
            (1, 10)
        );
    }
}
