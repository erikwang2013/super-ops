use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use crate::model::runbook::{
    create_runbook, create_runbook_run, delete_runbook, get_runbook, list_runbook_runs,
    list_runbooks, mark_runbook_run, validate_steps,
};
use crate::model::script::get_script_content;
use crate::model::tenant::Tenant;
use crate::proxy::k8s_proxy::status_to_http;
use crate::scripts_api::{build_runner_command, truncate_output};

#[derive(Debug, Deserialize)]
pub struct CreateRunbook {
    pub name: String,
    pub description: Option<String>,
    /// JSON 数组：[{"name":"检查","script_id":1,"timeout_s":30}]
    pub steps: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct RunRunbook {
    pub cluster_id: String,
    pub namespace: String,
    #[serde(default)]
    pub target_pods: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListRunsQuery {
    pub limit: Option<i64>,
}

pub async fn list_runbooks_handler(
    State(state): State<crate::AppState>,
    Extension(tenant): Extension<Tenant>,
) -> impl IntoResponse {
    match list_runbooks(&state.pool, &tenant.0).await {
        Ok(runbooks) => (
            StatusCode::OK,
            Json(serde_json::json!({ "runbooks": runbooks })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("runbook query failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn create_runbook_handler(
    State(state): State<crate::AppState>,
    Extension(tenant): Extension<Tenant>,
    Json(req): Json<CreateRunbook>,
) -> impl IntoResponse {
    let n = req.name.chars().count();
    if !(1..=128).contains(&n) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("name 长度需 1..=128，当前 {n}") })),
        )
            .into_response();
    }
    if let Err(msg) = validate_steps(&req.steps) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response();
    }
    match create_runbook(
        &state.pool,
        &tenant.0,
        &req.name,
        req.description.as_deref().unwrap_or(""),
        &req.steps,
        "",
    )
    .await
    {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("runbook write failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn delete_runbook_handler(
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
    Extension(tenant): Extension<Tenant>,
) -> impl IntoResponse {
    match delete_runbook(&state.pool, &tenant.0, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "runbook not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("runbook delete failed: {e}") })),
        )
            .into_response(),
    }
}

/// 按序执行剧本：每步解析脚本 → k8s RunJob；任一步失败即终止并记录 failed。
pub async fn run_runbook_handler(
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
    Extension(tenant): Extension<Tenant>,
    Json(req): Json<RunRunbook>,
) -> impl IntoResponse {
    if req.cluster_id.is_empty() || req.namespace.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "cluster_id/namespace 不能为空" })),
        )
            .into_response();
    }
    let runbook = match get_runbook(&state.pool, &tenant.0, id).await {
        Ok(Some(rb)) => rb,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "runbook not found" })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("runbook query failed: {e}") })),
            )
                .into_response();
        }
    };
    let steps = match validate_steps(&runbook.steps) {
        Ok(s) => s,
        Err(msg) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    };
    let pods: Vec<String> = req
        .target_pods
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    let target_pods_joined: String = pods.join(",").chars().take(512).collect();
    let run_id = match create_runbook_run(&state.pool, id, &target_pods_joined).await {
        Ok(rid) => rid,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("run create failed: {e}") })),
            )
                .into_response();
        }
    };
    let endpoint = { state.k8s_endpoint.read().unwrap().clone() };
    let client = match crate::k8s_client::connect(&endpoint, &state.k8s_token).await {
        Ok(c) => c,
        Err(e) => {
            let _ = mark_runbook_run(
                &state.pool,
                run_id,
                "failed",
                &format!("k8s backend unreachable: {e}"),
            )
            .await;
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("k8s backend unreachable: {e}") })),
            )
                .into_response();
        }
    };
    let mut client = client;
    let mut output = String::new();
    for (i, step) in steps.iter().enumerate() {
        let content = match get_script_content(&state.pool, &tenant.0, step.script_id).await {
            Ok(Some(c)) => c,
            Ok(None) => {
                output.push_str(&format!(
                    "[{}] failed: {}(script {}) 脚本不存在\n",
                    i + 1,
                    step.name,
                    step.script_id
                ));
                let _ = mark_runbook_run(&state.pool, run_id, "failed", &output).await;
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": format!("step {} 脚本 {} 不存在", step.name, step.script_id)
                    })),
                )
                    .into_response();
            }
            Err(e) => {
                output.push_str(&format!(
                    "[{}] failed: {}(script {}) 查询失败 {e}\n",
                    i + 1,
                    step.name,
                    step.script_id
                ));
                let _ = mark_runbook_run(&state.pool, run_id, "failed", &output).await;
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": format!("script query failed: {e}") })),
                )
                    .into_response();
            }
        };
        let command = build_runner_command(&pods, &content);
        let job_name = format!("superops-runbook-{id}-{run_id}-{i}");
        let result = client
            .run_job(superops_protos::k8s::v1::RunJobRequest {
                cluster_id: req.cluster_id.clone(),
                namespace: req.namespace.clone(),
                job_name,
                image: "busybox:1.36".into(),
                command,
                timeout_s: step.timeout_s,
            })
            .await
            .map_err(status_to_http);
        match result {
            Ok(_) => output.push_str(&format!(
                "[{}] ok: {}(script {})\n",
                i + 1,
                step.name,
                step.script_id
            )),
            Err((_, body)) => {
                let msg = body
                    .0
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                output.push_str(&format!("[{}] failed: {}: {}\n", i + 1, step.name, msg));
                let _ = mark_runbook_run(&state.pool, run_id, "failed", &output).await;
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({
                        "error": format!("step {} 执行失败", step.name),
                        "run_id": run_id,
                    })),
                )
                    .into_response();
            }
        }
    }
    let _ = mark_runbook_run(&state.pool, run_id, "ok", &output).await;
    (
        StatusCode::OK,
        Json(serde_json::json!({ "run_id": run_id, "status": "ok", "steps": steps.len() })),
    )
        .into_response()
}

pub async fn list_runbook_runs_handler(
    State(state): State<crate::AppState>,
    Query(q): Query<ListRunsQuery>,
    Extension(tenant): Extension<Tenant>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    match list_runbook_runs(&state.pool, &tenant.0, limit).await {
        Ok(mut runs) => {
            for r in &mut runs {
                if let Some(out) = &r.output {
                    r.output = Some(truncate_output(out, 100 * 1024));
                }
            }
            (StatusCode::OK, Json(serde_json::json!({ "runs": runs }))).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("run query failed: {e}") })),
        )
            .into_response(),
    }
}
