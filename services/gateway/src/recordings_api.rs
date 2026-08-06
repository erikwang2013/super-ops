use crate::AppState;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use ecat_data::RdbmsClient;
use serde::Deserialize;
use serde_json::Value;

type ApiError = (StatusCode, Json<Value>);

fn err(status: StatusCode, message: &str) -> ApiError {
    (status, Json(serde_json::json!({ "error": message })))
}

#[derive(Debug, Default, Deserialize)]
pub struct RecordingsQuery {
    pub limit: Option<i64>,
}

fn clamp_limit(q: &RecordingsQuery) -> i64 {
    q.limit.unwrap_or(50).clamp(1, 500)
}

fn esc_sid(s: &str) -> String {
    s.replace('\'', "''")
}

fn as_i64(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

// 会话 ID 由 recorder::session_id() 生成（UUID v4），此处按 8-4-4-4-12 形状校验，
// 从根上杜绝引号/反斜杠注入（esc_sid 仅作纵深防御）。
fn valid_sid(sid: &str) -> bool {
    let b = sid.as_bytes();
    b.len() == 36
        && matches!((b[8], b[13], b[18], b[23]), (b'-', b'-', b'-', b'-'))
        && b.iter()
            .enumerate()
            .all(|(i, &c)| matches!(i, 8 | 13 | 18 | 23) || c.is_ascii_hexdigit())
}

fn check_sid(sid: &str) -> Result<(), ApiError> {
    if !valid_sid(sid) {
        return Err(err(StatusCode::BAD_REQUEST, "invalid session_id"));
    }
    Ok(())
}

// 单次回放上限 5000 帧，防超长会话撑爆内存
fn frames_sql(sid: &str) -> String {
    format!(
        "SELECT timestamp, frame_b64 FROM exec_session WHERE kind = 'recording' AND session_id = '{}' ORDER BY timestamp LIMIT 5000",
        esc_sid(sid)
    )
}

/// GET /api/recordings?limit=50 —— 按 session_id 聚合列出录制会话
pub async fn list_recordings(
    State(state): State<AppState>,
    Query(q): Query<RecordingsQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = clamp_limit(&q);
    let sql = format!(
        "SELECT session_id, min(timestamp) AS created_at, count() AS frames FROM exec_session WHERE kind = 'recording' GROUP BY session_id ORDER BY created_at DESC LIMIT {limit}"
    );
    let rows = state.ch.query(&sql).await.map_err(|e| {
        err(
            StatusCode::BAD_GATEWAY,
            &format!("recordings query failed: {e}"),
        )
    })?;
    let recordings: Vec<Value> = rows
        .iter()
        .filter_map(|r| {
            let sid = r.get("session_id")?.as_str()?.to_string();
            Some(serde_json::json!({
                "session_id": sid,
                "created_at": r.get("created_at").and_then(as_i64).unwrap_or(0),
                "frames": r.get("frames").and_then(as_i64).unwrap_or(0),
            }))
        })
        .collect();
    Ok(Json(serde_json::json!({ "recordings": recordings })))
}

/// GET /api/recordings/{sid}/frames —— 按时间序返回该会话全部帧（base64，前端回放）
pub async fn get_frames(
    State(state): State<AppState>,
    Path(sid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    check_sid(&sid)?;
    let rows = state.ch.query(&frames_sql(&sid)).await.map_err(|e| {
        err(
            StatusCode::BAD_GATEWAY,
            &format!("frames query failed: {e}"),
        )
    })?;
    let frames: Vec<Value> = rows
        .iter()
        .filter_map(|r| {
            Some(serde_json::json!({
                "ts": r.get("timestamp").and_then(as_i64).unwrap_or(0),
                "data": r.get("frame_b64")?.as_str().unwrap_or(""),
            }))
        })
        .collect();
    Ok(Json(
        serde_json::json!({ "session_id": sid, "frames": frames }),
    ))
}

/// DELETE /api/recordings/{sid} —— ClickHouse 轻量删除该会话全部录制帧
pub async fn delete_recording(
    State(state): State<AppState>,
    Path(sid): Path<String>,
) -> Result<StatusCode, ApiError> {
    check_sid(&sid)?;
    // TsdbClient 与 RdbmsClient 均有 query 方法（返回类型不同），delete 仅 TsdbClient 提供，
    // 故用 UFCS 调用避免与 RdbmsClient::query 产生方法解析歧义
    ecat_data::TsdbClient::delete(
        state.ch.as_ref(),
        &format!(
            "ALTER TABLE exec_session DELETE WHERE kind = 'recording' AND session_id = '{}'",
            esc_sid(&sid)
        ),
    )
    .await
    .map_err(|e| err(StatusCode::BAD_GATEWAY, &format!("delete failed: {e}")))?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_sid_accepts_uuid_shape() {
        assert!(valid_sid("123e4567-e89b-42d3-a456-426614174000"));
    }

    #[test]
    fn valid_sid_rejects_quotes_backslash_and_garbage() {
        assert!(!valid_sid("123e4567-e89b-42d3-a456-4266141740'0"));
        assert!(!valid_sid("123e4567-e89b-42d3-a456-4266141740\\0"));
        assert!(!valid_sid("../etc/passwd"));
        assert!(!valid_sid(&"a".repeat(37)));
        assert!(!valid_sid(""));
    }

    #[test]
    fn esc_sid_doubles_quotes() {
        assert_eq!(esc_sid("a'b"), "a''b");
        assert_eq!(esc_sid("\\"), "\\");
        assert_eq!(esc_sid(""), "");
    }

    #[test]
    fn clamp_limit_bounds() {
        let q = |n: Option<i64>| RecordingsQuery { limit: n };
        assert_eq!(clamp_limit(&q(None)), 50);
        assert_eq!(clamp_limit(&q(Some(1))), 1);
        assert_eq!(clamp_limit(&q(Some(0))), 1);
        assert_eq!(clamp_limit(&q(Some(501))), 500);
        assert_eq!(clamp_limit(&q(Some(9999))), 500);
    }

    #[test]
    fn frames_sql_caps_and_escapes() {
        assert!(frames_sql("x").ends_with("LIMIT 5000"));
        assert!(frames_sql("a'b").contains("session_id = 'a''b'"));
    }
}
