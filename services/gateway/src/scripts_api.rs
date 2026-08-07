use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use crate::model::script::{
    create_run, create_script, delete_script, get_script, get_script_content, list_runs,
    list_scripts, mark_run_status, validate_script_input, validate_timeout_s,
};
use crate::model::tenant::Tenant;
use crate::proxy::k8s_proxy::status_to_http;

#[derive(Debug, Default, Deserialize)]
pub struct CreateScript {
    pub name: String,
    pub description: Option<String>,
    pub language: Option<String>,
    pub content: String,
    pub timeout_s: Option<i32>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RunScript {
    pub cluster_id: String,
    pub namespace: String,
    #[serde(default)]
    pub target_pods: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListRunsQuery {
    pub script_id: Option<i64>,
    pub limit: Option<i64>,
}

pub async fn list_scripts_handler(
    State(state): State<crate::AppState>,
    Extension(tenant): Extension<Tenant>,
) -> impl IntoResponse {
    match list_scripts(&state.pool, &tenant.0).await {
        Ok(scripts) => (
            StatusCode::OK,
            Json(serde_json::json!({ "scripts": scripts })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("script query failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn create_script_handler(
    State(state): State<crate::AppState>,
    Extension(tenant): Extension<Tenant>,
    Json(req): Json<CreateScript>,
) -> impl IntoResponse {
    let language = req.language.unwrap_or_else(|| "shell".into());
    if let Err(msg) = validate_script_input(&req.name, &language, &req.content) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response();
    }
    if let Some(t) = req.timeout_s {
        if let Err(msg) = validate_timeout_s(t) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response();
        }
    }
    match create_script(
        &state.pool,
        &tenant.0,
        &req.name,
        req.description.as_deref().unwrap_or(""),
        &language,
        &req.content,
        req.timeout_s.unwrap_or(300),
        "",
    )
    .await
    {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("script write failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn delete_script_handler(
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
    Extension(tenant): Extension<Tenant>,
) -> impl IntoResponse {
    match delete_script(&state.pool, &tenant.0, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "script not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("script delete failed: {e}") })),
        )
            .into_response(),
    }
}

/// Pod 名仅允许 DNS-1123 字符（小写字母/数字/点/连字符），避免注入 shell。
fn sanitize_pod_name(pod: &str) -> String {
    pod.chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-' || *c == '.')
        .take(63)
        .collect()
}

pub fn build_runner_command(pods: &[String], content: &str) -> String {
    let mut head = String::new();
    for p in pods {
        head.push_str(&format!(
            "echo \"=== target pod: {} ===\"\n",
            sanitize_pod_name(p)
        ));
    }
    if pods.is_empty() {
        head.push_str("for p in $(echo \"none\") ; do :; done\n");
    }
    format!("set -e\n{head}{content}")
}

pub async fn run_script_handler(
    State(state): State<crate::AppState>,
    Path(script_id): Path<i64>,
    Extension(tenant): Extension<Tenant>,
    Json(req): Json<RunScript>,
) -> impl IntoResponse {
    let script = match get_script(&state.pool, &tenant.0, script_id).await {
        Ok(Some(s)) => {
            if s.language != "shell" {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "仅支持 shell 脚本执行（runner 镜像固定 busybox:1.36）"
                    })),
                )
                    .into_response();
            }
            s
        }
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "script not found" })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("script query failed: {e}") })),
            )
                .into_response();
        }
    };
    if req.cluster_id.is_empty() || req.namespace.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "cluster_id/namespace must not be empty" })),
        )
            .into_response();
    }
    let pods: Vec<String> = req
        .target_pods
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    let target_pods_joined: String = pods.join(",").chars().take(512).collect();
    let content = match get_script_content(&state.pool, &tenant.0, script_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "script not found" })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("script query failed: {e}") })),
            )
                .into_response();
        }
    };
    let run_id = match create_run(&state.pool, script_id, &target_pods_joined).await {
        Ok(id) => id,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("run create failed: {e}") })),
            )
                .into_response();
        }
    };
    let job_name = format!("superops-script-{script_id}-{run_id}");
    let command = build_runner_command(&pods, &content);
    let endpoint = { state.k8s_endpoint.read().unwrap().clone() };
    let client = superops_protos::k8s::v1::k8s_service_client::K8sServiceClient::connect(endpoint)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("k8s backend unreachable: {e}") })),
            )
        });
    let mut client = match client {
        Ok(c) => c,
        Err((code, body)) => {
            let _ = mark_run_status(&state.pool, run_id, "failed").await;
            return (code, body).into_response();
        }
    };
    let result = client
        .run_job(superops_protos::k8s::v1::RunJobRequest {
            cluster_id: req.cluster_id.clone(),
            namespace: req.namespace.clone(),
            job_name,
            image: "busybox:1.36".into(),
            command,
            timeout_s: script.timeout_s,
        })
        .await
        .map_err(status_to_http);
    match result {
        Ok(_) => {
            let _ = mark_run_status(&state.pool, run_id, "running").await;
            (
                StatusCode::OK,
                Json(serde_json::json!({ "run_id": run_id, "status": "running" })),
            )
                .into_response()
        }
        Err((code, body)) => {
            let _ = mark_run_status(&state.pool, run_id, "failed").await;
            (code, body).into_response()
        }
    }
}

/// 截断到 ≤ max 字节的最后一个 UTF-8 字符边界（多字节字符不被切断）。
pub fn truncate_output(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let end = s
        .char_indices()
        .take_while(|(i, _)| *i <= max)
        .map(|(i, _)| i)
        .last()
        .unwrap_or(0);
    s[..end].to_string()
}

pub async fn list_runs_handler(
    State(state): State<crate::AppState>,
    Query(q): Query<ListRunsQuery>,
    Extension(tenant): Extension<Tenant>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    match list_runs(&state.pool, &tenant.0, q.script_id, limit).await {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_multibyte_boundary() {
        let s = "中".repeat(500);
        let out = truncate_output(&s, 64);
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
        assert!(out.len() <= 64);
        assert_eq!(out, truncate_output(&s, 64));
    }

    #[test]
    fn truncate_short_string_untouched() {
        assert_eq!(truncate_output("hello", 64), "hello");
        assert_eq!(truncate_output("", 64), "");
    }

    #[test]
    fn build_command_noop_without_pods() {
        let cmd = build_runner_command(&[], "echo hi");
        assert!(cmd.starts_with("set -e\n"));
        assert!(cmd.contains("echo hi"));
        assert!(cmd.contains("none"));
    }

    #[test]
    fn build_command_echoes_pods() {
        let cmd = build_runner_command(&["pod-1".into(), "pod_2!x".into()], "echo hi");
        assert!(cmd.contains("=== target pod: pod-1 ==="));
        // 非法字符被过滤（_ 与 ! 被移除）
        assert!(cmd.contains("=== target pod: pod2x ==="));
        assert!(!cmd.contains("pod_2!x"));
        assert!(cmd.ends_with("echo hi"));
    }
}
