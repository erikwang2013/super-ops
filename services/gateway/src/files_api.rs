use crate::AppState;
use crate::recorder::valid_filename;
use axum::{
    Json,
    extract::{Multipart, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
};
use serde_json::Value;

type ApiError = (StatusCode, Json<Value>);

fn err(status: StatusCode, message: &str) -> ApiError {
    (status, Json(serde_json::json!({ "error": message })))
}

const UPLOAD_DIR: &str = "data/uploads";
const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;

/// POST /api/files —— multipart 单文件上传：valid_filename + 1B..=10MB，写入 data/uploads/{name}
pub async fn upload_file(
    State(_state): State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let mut uploaded: Option<(String, Vec<u8>)> = None;
    while let Some(field) = multipart.next_field().await.map_err(|e| {
        err(
            StatusCode::BAD_REQUEST,
            &format!("multipart read failed: {e}"),
        )
    })? {
        if field.name() != Some("file") {
            continue;
        }
        let name = field.file_name().unwrap_or_default().to_string();
        let bytes = field
            .bytes()
            .await
            .map_err(|e| err(StatusCode::BAD_REQUEST, &format!("file read failed: {e}")))?
            .to_vec();
        if !valid_filename(&name) {
            return Err(err(
                StatusCode::BAD_REQUEST,
                "filename must be 1-255 chars: letters, digits, '.', '_' or '-'",
            ));
        }
        if bytes.is_empty() || bytes.len() > MAX_FILE_BYTES {
            return Err(err(StatusCode::BAD_REQUEST, "file must be 1B..=10MB"));
        }
        uploaded = Some((name, bytes));
        break;
    }
    let (name, bytes) =
        uploaded.ok_or_else(|| err(StatusCode::BAD_REQUEST, "missing 'file' field"))?;
    tokio::fs::create_dir_all(UPLOAD_DIR).await.map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("upload dir failed: {e}"),
        )
    })?;
    tokio::fs::write(format!("{UPLOAD_DIR}/{name}"), bytes)
        .await
        .map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("file write failed: {e}"),
            )
        })?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "name": name })),
    ))
}

/// GET /api/files/{name} —— 二次 valid_filename 校验后读取，任何异常一律 404（防路径穿越探测）
pub async fn get_file(
    State(_state): State<AppState>,
    Path(name): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if !valid_filename(&name) {
        return Err(err(StatusCode::NOT_FOUND, "file not found"));
    }
    let data = tokio::fs::read(format!("{UPLOAD_DIR}/{name}"))
        .await
        .map_err(|_| err(StatusCode::NOT_FOUND, "file not found"))?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/octet-stream"),
    );
    Ok((StatusCode::OK, headers, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_whitelist() {
        assert!(valid_filename("report-2026.pdf"));
        assert!(valid_filename("a.b_c-1.txt"));
        assert!(!valid_filename("../etc/passwd"));
        assert!(!valid_filename("a/b"));
        assert!(!valid_filename(""));
        assert!(!valid_filename(&"x".repeat(256)));
    }
}
