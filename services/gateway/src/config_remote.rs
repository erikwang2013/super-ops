use ecat_config::ConfigError;
use ecat_middleware::RateLimitStore;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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
    if let Some(v) = values.get("rate.limit.max") {
        if let Some(max) = v.as_u64() {
            *cfg.rate_limit_max.write().unwrap() = max as u32;
            applied.push(format!("rate.limit.max={max}"));
        }
    }
    if let Some(v) = values.get("rate.limit.window") {
        if let Some(secs) = v.as_u64() {
            *cfg.rate_limit_window_secs.write().unwrap() = secs;
            applied.push(format!("rate.limit.window={secs}"));
        }
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
}
