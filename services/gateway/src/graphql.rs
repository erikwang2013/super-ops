use crate::AppState;
use ecat_data::RdbmsClient;
use ecat_graphql::GraphQLSchema;
use std::sync::Arc;

/// GraphQL 根查询解析器：health / cmdbStats / alerts / backups。
/// 解析器闭包捕获 AppState，variables 按 GraphQL 约定传入（当前均忽略）。
pub fn build_schema(state: AppState) -> GraphQLSchema {
    let s = Arc::new(state);
    GraphQLSchema::new()
        .query_fn("health", |_vars| {
            Box::pin(async {
                Ok(serde_json::json!({
                    "status": "ok",
                    "service": "superops-gateway",
                    "version": env!("CARGO_PKG_VERSION"),
                }))
            })
        })
        .query_fn("cmdbStats", {
            let s = Arc::clone(&s);
            move |_vars| {
                let s = Arc::clone(&s);
                Box::pin(async move {
                    let rows: Vec<(String, i64)> = sqlx::query_as(
                        "SELECT asset_type, COUNT(*) AS cnt FROM cmdb_asset GROUP BY asset_type",
                    )
                    .fetch_all(&s.pool)
                    .await
                    .map_err(|e| format!("cmdb stats query failed: {e}"))?;
                    let total: i64 = rows.iter().map(|(_, c)| c).sum();
                    let by_type: serde_json::Map<String, serde_json::Value> = rows
                        .into_iter()
                        .map(|(t, c)| (t, serde_json::json!(c)))
                        .collect();
                    Ok(serde_json::json!({ "total": total, "by_type": by_type }))
                })
            }
        })
        .query_fn("alerts", {
            let s = Arc::clone(&s);
            move |_vars| {
                let s = Arc::clone(&s);
                Box::pin(async move {
                    let sql = "SELECT level, title, message, \
                               formatDateTime(toDateTime(timestamp), '%Y-%m-%d %H:%i:%s') AS ts \
                               FROM alert_event ORDER BY timestamp DESC LIMIT 100";
                    let rows = s
                        .ch
                        .query(sql)
                        .await
                        .map_err(|e| format!("alerts query failed: {e}"))?;
                    let alerts: Vec<serde_json::Value> = rows
                        .iter()
                        .map(|r| {
                            let mut m = serde_json::Map::new();
                            for col in ["level", "title", "message", "ts"] {
                                if let Some(v) = r.get(col) {
                                    m.insert(col.to_string(), v.clone());
                                }
                            }
                            serde_json::Value::Object(m)
                        })
                        .collect();
                    Ok(serde_json::json!(alerts))
                })
            }
        })
        .query_fn("backups", {
            let s = Arc::clone(&s);
            move |_vars| {
                let s = Arc::clone(&s);
                Box::pin(async move {
                    let counts: Vec<(String, i64)> = sqlx::query_as(
                        "SELECT status, COUNT(*) AS cnt FROM backup_status GROUP BY status",
                    )
                    .fetch_all(&s.pool)
                    .await
                    .map_err(|e| format!("backups query failed: {e}"))?;
                    let latest: Vec<(String, String, String, i64)> = sqlx::query_as(
                        "SELECT db_name, status, target, size_bytes FROM backup_status \
                         ORDER BY id DESC LIMIT 10",
                    )
                    .fetch_all(&s.pool)
                    .await
                    .map_err(|e| format!("backups latest query failed: {e}"))?;
                    let by_status: serde_json::Map<String, serde_json::Value> = counts
                        .into_iter()
                        .map(|(k, v)| (k, serde_json::json!(v)))
                        .collect();
                    let recent: Vec<serde_json::Value> = latest
                        .into_iter()
                        .map(|(db_name, status, target, size_bytes)| {
                            serde_json::json!({
                                "db_name": db_name,
                                "status": status,
                                "target": target,
                                "size_bytes": size_bytes,
                            })
                        })
                        .collect();
                    Ok(serde_json::json!({ "by_status": by_status, "recent": recent }))
                })
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ecat_graphql::graphql_router;

    #[test]
    fn schema_router_builds() {
        let schema = GraphQLSchema::new().query_fn("ping", |_vars| {
            Box::pin(async { Ok(serde_json::json!("pong")) })
        });
        let _router: axum::Router<()> = graphql_router(schema);
    }
}
