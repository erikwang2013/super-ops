use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use base64::Engine;
use ecat_config::ConfigError;
use ecat_middleware::RateLimitStore;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Consul KV 管理 API（D3 配置中心）：读写 config/superops 前缀下的键。
const KV_PREFIX: &str = "config/superops";

fn validate_key(key: &str) -> Result<(), String> {
    if !key.starts_with(KV_PREFIX) {
        return Err(format!("key must start with '{KV_PREFIX}/'"));
    }
    if key.len() <= KV_PREFIX.len() + 1 {
        return Err("key must include a name after the prefix".into());
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct PutConfigKey {
    pub value: String,
}

pub async fn list_config_keys(
    State(state): State<crate::AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let Some(addr) = &state.consul else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "consul not configured"})),
        ));
    };
    let url = format!("{addr}/v1/kv/{KV_PREFIX}?recurse=true");
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| backend_error(&e.to_string()))?;
    if resp.status().is_success() {
        let raw: Vec<serde_json::Value> = resp
            .json()
            .await
            .map_err(|e| backend_error(&e.to_string()))?;
        let keys: Vec<serde_json::Value> = raw
            .iter()
            .map(|e| {
                let key = e["Key"].as_str().unwrap_or_default();
                let value = e["Value"]
                    .as_str()
                    .and_then(|v| base64::engine::general_purpose::STANDARD.decode(v).ok())
                    .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                    .unwrap_or_default();
                serde_json::json!({
                    "key": key,
                    "value": value,
                    "modified_index": e["ModifyIndex"].as_u64().unwrap_or(0),
                })
            })
            .collect();
        Ok(Json(serde_json::json!({ "keys": keys })))
    } else {
        Err(backend_error(&format!(
            "consul list http {}",
            resp.status()
        )))
    }
}

pub async fn get_config_key(
    State(state): State<crate::AppState>,
    Path(key): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    validate_key(&key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
    })?;
    let Some(addr) = &state.consul else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "consul not configured"})),
        ));
    };
    let url = format!("{addr}/v1/kv/{key}?raw=true");
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| backend_error(&e.to_string()))?;
    if resp.status() == StatusCode::NOT_FOUND {
        return Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "key not found"})),
        ));
    }
    if !resp.status().is_success() {
        return Err(backend_error(&format!("consul get http {}", resp.status())));
    }
    let value = resp
        .text()
        .await
        .map_err(|e| backend_error(&e.to_string()))?;
    Ok(Json(serde_json::json!({ "key": key, "value": value })))
}

pub async fn put_config_key(
    State(state): State<crate::AppState>,
    Path(key): Path<String>,
    Json(body): Json<PutConfigKey>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    validate_key(&key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
    })?;
    let Some(addr) = &state.consul else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "consul not configured"})),
        ));
    };
    let client = reqwest::Client::new();
    let resp = client
        .put(format!("{addr}/v1/kv/{key}"))
        .body(body.value)
        .header("Content-Type", "text/plain")
        .send()
        .await
        .map_err(|e| backend_error(&e.to_string()))?;
    if resp.status().is_success() {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(backend_error(&format!("consul put http {}", resp.status())))
    }
}

pub async fn delete_config_key(
    State(state): State<crate::AppState>,
    Path(key): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    validate_key(&key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e})),
        )
    })?;
    let Some(addr) = &state.consul else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "consul not configured"})),
        ));
    };
    let client = reqwest::Client::new();
    let resp = client
        .delete(format!("{addr}/v1/kv/{key}"))
        .send()
        .await
        .map_err(|e| backend_error(&e.to_string()))?;
    if resp.status().is_success() {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(backend_error(&format!(
            "consul delete http {}",
            resp.status()
        )))
    }
}

fn backend_error(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({"error": format!("consul backend: {msg}")})),
    )
}

pub struct DynamicConfig {
    pub rate_limit_max: RwLock<u32>,
    pub rate_limit_window_secs: RwLock<u64>,
}

impl Default for DynamicConfig {
    fn default() -> Self {
        Self {
            rate_limit_max: RwLock::new(10),
            rate_limit_window_secs: RwLock::new(60),
        }
    }
}

pub fn apply_dynamic(
    values: &HashMap<String, serde_json::Value>,
    cfg: &DynamicConfig,
) -> Vec<String> {
    let mut applied = Vec::new();
    if let Some(v) = values.get("rate.limit.max")
        && let Some(max) = v.as_u64()
    {
        *cfg.rate_limit_max.write().unwrap() = max as u32;
        applied.push(format!("rate.limit.max={max}"));
    }
    if let Some(v) = values.get("rate.limit.window")
        && let Some(secs) = v.as_u64()
    {
        *cfg.rate_limit_window_secs.write().unwrap() = secs;
        applied.push(format!("rate.limit.window={secs}"));
    }
    applied
}

pub struct DynamicRateLimitStore {
    inner: Arc<dyn RateLimitStore>,
    dynamic: Arc<DynamicConfig>,
}

impl DynamicRateLimitStore {
    pub fn new(inner: Arc<dyn RateLimitStore>, dynamic: Arc<DynamicConfig>) -> Self {
        Self { inner, dynamic }
    }
}

#[async_trait::async_trait]
impl RateLimitStore for DynamicRateLimitStore {
    async fn check(&self, key: &str, _max: u32, _window_secs: u64) -> Result<(), String> {
        let max = *self.dynamic.rate_limit_max.read().unwrap();
        let window_secs = *self.dynamic.rate_limit_window_secs.read().unwrap();
        self.inner.check(key, max, window_secs).await
    }
}

pub async fn run_config_watcher(
    mut rx: tokio::sync::mpsc::Receiver<Result<HashMap<String, serde_json::Value>, ConfigError>>,
    dynamic: Arc<DynamicConfig>,
) {
    while let Some(update) = rx.recv().await {
        match update {
            Ok(values) => {
                let applied = apply_dynamic(&values, &dynamic);
                if !applied.is_empty() {
                    tracing::info!(applied = ?applied, "dynamic config applied");
                }
            }
            Err(e) => tracing::warn!(error = %e, "config watch error"),
        }
    }
    tracing::info!("config watcher channel closed; exiting");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let cfg = DynamicConfig::default();
        assert_eq!(*cfg.rate_limit_max.read().unwrap(), 10);
        assert_eq!(*cfg.rate_limit_window_secs.read().unwrap(), 60);
    }

    #[test]
    fn apply_updates_values() {
        let cfg = DynamicConfig::default();
        let mut values = HashMap::new();
        values.insert("rate.limit.max".into(), serde_json::json!(20));
        values.insert("rate.limit.window".into(), serde_json::json!(30));
        let applied = apply_dynamic(&values, &cfg);
        assert_eq!(applied.len(), 2);
        assert_eq!(*cfg.rate_limit_max.read().unwrap(), 20);
        assert_eq!(*cfg.rate_limit_window_secs.read().unwrap(), 30);
    }

    #[test]
    fn apply_ignores_unknown_and_bad_types() {
        let cfg = DynamicConfig::default();
        let mut values = HashMap::new();
        values.insert("rate.limit.max".into(), serde_json::json!("abc"));
        values.insert("unknown.key".into(), serde_json::json!(5));
        let applied = apply_dynamic(&values, &cfg);
        assert!(applied.is_empty());
        assert_eq!(*cfg.rate_limit_max.read().unwrap(), 10);
        assert_eq!(*cfg.rate_limit_window_secs.read().unwrap(), 60);
    }

    #[tokio::test]
    async fn dynamic_store_delegates_with_live_values() {
        let dynamic = Arc::new(DynamicConfig::default());
        *dynamic.rate_limit_max.write().unwrap() = 2;
        let inner: Arc<dyn RateLimitStore> = Arc::new(ecat_middleware::MemoryStore::new());
        let store = DynamicRateLimitStore::new(inner, dynamic);
        assert!(store.check("ip-1", 0, 0).await.is_ok());
        assert!(store.check("ip-1", 0, 0).await.is_ok());
        assert!(store.check("ip-1", 0, 0).await.is_err());
    }

    #[test]
    fn kv_validate_key_requires_prefix() {
        assert!(validate_key("config/superops/gateway/rate.limit.max").is_ok());
        assert!(validate_key("config/superops").is_err());
        assert!(validate_key("other/prefix/x").is_err());
        assert!(validate_key("").is_err());
    }
}
