use crate::model::cmdb::{AssetRow, list_assets};
use crate::model::tenant::Tenant;
use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
    response::IntoResponse,
};
use ecat_data::GraphClient;
use serde::{Deserialize, Serialize};

const MAX_QUERY_CHARS: usize = 4096;

#[derive(Debug, Clone, Serialize)]
pub struct TopoNode {
    pub key: String,
    pub asset_type: String,
    pub name: String,
    pub ip: Option<String>,
    pub env: String,
    pub owner: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopoEdge {
    pub src: String,
    pub dst: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Topology {
    pub nodes: Vec<TopoNode>,
    pub edges: Vec<TopoEdge>,
}

fn asset_key(tenant: &str, name: &str) -> String {
    format!("{tenant}:{name}")
}

/// 从资产 labels JSON 中提取 depends_on 依赖列表（非法 JSON 视为无依赖）。
fn depends_on(labels: Option<&str>) -> Vec<String> {
    let Some(labels) = labels else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(labels) else {
        return Vec::new();
    };
    v.get("depends_on")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.chars().take(128).collect()))
                .collect()
        })
        .unwrap_or_default()
}

fn merge_node_query() -> &'static str {
    "MERGE (a:Asset {key: $key}) \
     SET a.tenant = $tenant, a.asset_type = $asset_type, a.name = $name, \
         a.ip = $ip, a.env = $env, a.owner = $owner, a.status = $status"
}

fn merge_edge_query() -> &'static str {
    "MATCH (a:Asset {key: $src}), (b:Asset {key: $dst}) \
     MERGE (a)-[:DEPENDS_ON]->(b)"
}

fn prune_query() -> &'static str {
    "MATCH (a:Asset {tenant: $tenant}) \
     WHERE NOT a.key IN $keys \
     DETACH DELETE a"
}

fn fetch_query() -> &'static str {
    "MATCH (a:Asset {tenant: $tenant}) \
     OPTIONAL MATCH (a)-[:DEPENDS_ON]->(b:Asset {tenant: $tenant}) \
     RETURN a.key AS src, b.key AS dst, \
            a.asset_type AS st, a.name AS sn, a.ip AS si, \
            a.env AS se, a.owner AS so, a.status AS ss"
}

/// Neo4j 事务端点返回 {"results":[{"data":[{"row":[...]}, ...]}]}。
fn parse_neo4j_rows(result: &serde_json::Value) -> Vec<Vec<serde_json::Value>> {
    result
        .pointer("/results/0/data")
        .and_then(serde_json::Value::as_array)
        .map(|data| {
            data.iter()
                .filter_map(|d| d.get("row").and_then(serde_json::Value::as_array).cloned())
                .collect()
        })
        .unwrap_or_default()
}

fn build_topology(result: &serde_json::Value) -> Topology {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for row in parse_neo4j_rows(result) {
        let src = row.first().and_then(serde_json::Value::as_str).unwrap_or("");
        if let Some(dst) = row.get(1).and_then(serde_json::Value::as_str) {
            edges.push(TopoEdge {
                src: src.to_string(),
                dst: dst.to_string(),
            });
        }
        if !src.is_empty() && seen.insert(src.to_string()) {
            let mut node = TopoNode {
                key: src.to_string(),
                asset_type: String::new(),
                name: String::new(),
                ip: None,
                env: String::new(),
                owner: String::new(),
                status: String::new(),
            };
            if let Some(t) = row.get(2).and_then(serde_json::Value::as_str) {
                node.asset_type = t.to_string();
            }
            if let Some(n) = row.get(3).and_then(serde_json::Value::as_str) {
                node.name = n.to_string();
            }
            node.ip = row.get(4).and_then(serde_json::Value::as_str).map(|s| s.to_string());
            if let Some(e) = row.get(5).and_then(serde_json::Value::as_str) {
                node.env = e.to_string();
            }
            if let Some(o) = row.get(6).and_then(serde_json::Value::as_str) {
                node.owner = o.to_string();
            }
            if let Some(s) = row.get(7).and_then(serde_json::Value::as_str) {
                node.status = s.to_string();
            }
            nodes.push(node);
        }
    }
    Topology { nodes, edges }
}

async fn upsert_node(
    graph: &dyn GraphClient,
    tenant: &str,
    a: &AssetRow,
) -> Result<(), String> {
    let params = serde_json::json!({
        "key": asset_key(tenant, &a.name),
        "tenant": tenant,
        "asset_type": a.asset_type,
        "name": a.name,
        "ip": a.ip,
        "env": a.env,
        "owner": a.owner,
        "status": a.status,
    });
    graph
        .execute(merge_node_query(), &params)
        .await
        .map(|_| ())
        .map_err(|e| format!("graph upsert {}: {e}", a.name))
}

async fn create_edge(
    graph: &dyn GraphClient,
    tenant: &str,
    src: &str,
    dst: &str,
) -> Result<(), String> {
    let params = serde_json::json!({
        "src": asset_key(tenant, src),
        "dst": asset_key(tenant, dst),
    });
    graph
        .execute(merge_edge_query(), &params)
        .await
        .map(|_| ())
        .map_err(|e| format!("graph edge {src}->{dst}: {e}"))
}

#[derive(Debug, Deserialize)]
pub struct ExploreReq {
    query: String,
    #[serde(default)]
    params: serde_json::Value,
}

pub async fn sync_topology(
    State(state): State<crate::AppState>,
    Extension(tenant): Extension<Tenant>,
) -> impl IntoResponse {
    let Some(graph) = &state.graph else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "graph 未配置（gateway.yaml graph 段）" })),
        )
            .into_response();
    };
    if !state.graph_is_neo4j {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!(
                "provider {} 仅支持 /api/cmdb/topology/explore 原生查询",
                state.graph_provider
            ) })),
        )
            .into_response();
    }
    let assets = match list_assets(&state.pool, &tenant.0, None, None).await {
        Ok(a) => a,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("cmdb query failed: {e}") })),
            )
                .into_response();
        }
    };
    let mut edge_count = 0usize;
    for a in &assets {
        if let Err(msg) = upsert_node(graph.as_ref(), &tenant.0, a).await {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
        for dst in depends_on(a.labels.as_deref()) {
            if let Err(msg) = create_edge(graph.as_ref(), &tenant.0, &a.name, &dst).await {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": msg })),
                )
                    .into_response();
            }
            edge_count += 1;
        }
    }
    let keys: Vec<String> = assets
        .iter()
        .map(|a| asset_key(&tenant.0, &a.name))
        .collect();
    let prune_params = serde_json::json!({ "tenant": tenant.0, "keys": keys });
    if let Err(e) = graph.execute(prune_query(), &prune_params).await {
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("graph prune: {e}") })),
        )
            .into_response();
    }
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "synced": assets.len(),
            "edges": edge_count,
            "provider": state.graph_provider,
        })),
    )
        .into_response()
}

pub async fn get_topology(
    State(state): State<crate::AppState>,
    Extension(tenant): Extension<Tenant>,
) -> impl IntoResponse {
    let Some(graph) = &state.graph else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "graph 未配置（gateway.yaml graph 段）" })),
        )
            .into_response();
    };
    if !state.graph_is_neo4j {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!(
                "provider {} 仅支持 /api/cmdb/topology/explore 原生查询",
                state.graph_provider
            ) })),
        )
            .into_response();
    }
    let params = serde_json::json!({ "tenant": tenant.0 });
    match graph.execute(fetch_query(), &params).await {
        Ok(result) => {
            let topo = build_topology(&result);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "provider": state.graph_provider,
                    "nodes": topo.nodes,
                    "edges": topo.edges,
                })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("graph query failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn explore_graph(
    State(state): State<crate::AppState>,
    Extension(_tenant): Extension<Tenant>,
    Json(req): Json<ExploreReq>,
) -> impl IntoResponse {
    let Some(graph) = &state.graph else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "graph 未配置（gateway.yaml graph 段）" })),
        )
            .into_response();
    };
    if req.query.is_empty() || req.query.chars().count() > MAX_QUERY_CHARS {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("query 需 1..={MAX_QUERY_CHARS} 字符") })),
        )
            .into_response();
    }
    if !req.params.is_object() && !req.params.is_null() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "params 必须为 JSON 对象" })),
        )
            .into_response();
    }
    match graph.execute(&req.query, &req.params).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("graph query failed: {e}") })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(
        src: &str,
        dst: Option<&str>,
        t: &str,
        name: &str,
        ip: Option<&str>,
        env: &str,
        owner: &str,
        status: &str,
    ) -> serde_json::Value {
        serde_json::json!({ "row": [src, dst, t, name, ip, env, owner, status] })
    }

    #[test]
    fn topology_normalizes_rows_and_dedups_nodes() {
        let result = serde_json::json!({
            "results": [{
                "columns": ["src", "dst", "st", "sn", "si", "se", "so", "ss"],
                "data": [
                    row("t:app-a", None, "app", "app-a", None, "prod", "erik", "active"),
                    row("t:app-a", Some("t:db-m"), "app", "app-a", None, "prod", "erik", "active"),
                    row("t:db-m", None, "db", "db-m", Some("10.0.0.1"), "prod", "", "active"),
                ]
            }]
        });
        let topo = build_topology(&result);
        assert_eq!(topo.nodes.len(), 2);
        assert_eq!(topo.edges.len(), 1);
        assert_eq!(topo.edges[0].src, "t:app-a");
        assert_eq!(topo.edges[0].dst, "t:db-m");
        assert_eq!(topo.nodes[0].name, "app-a");
        assert_eq!(topo.nodes[0].ip, None);
        assert_eq!(topo.nodes[1].ip, Some("10.0.0.1".into()));
    }

    #[test]
    fn depends_on_parses_labels_json() {
        assert_eq!(
            depends_on(Some(r#"{"depends_on":["db-m","cache-r"]}"#)),
            vec!["db-m", "cache-r"]
        );
        assert_eq!(depends_on(Some("not-json")), Vec::<String>::new());
        assert_eq!(depends_on(None), Vec::<String>::new());
        assert_eq!(depends_on(Some(r#"{"owner":"x"}"#)), Vec::<String>::new());
    }

    #[test]
    fn asset_key_separates_tenants() {
        assert_eq!(asset_key("t1", "web"), "t1:web");
        assert_ne!(asset_key("t1", "web"), asset_key("t2", "web"));
    }
}
